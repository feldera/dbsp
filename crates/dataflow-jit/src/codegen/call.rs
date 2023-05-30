use crate::{
    codegen::{
        intrinsics::TRIG_INTRINSICS, utils::FunctionBuilderExt, CodegenCtx, VTable, TRAP_ABORT,
        TRAP_ASSERT_EQ, TRAP_FAILED_PARSE,
    },
    ir::{exprs::Call, ColumnType, ExprId},
    ThinStr,
};
use cranelift::prelude::{types, FloatCC, FunctionBuilder, InstBuilder, IntCC, MemFlags};
use cranelift_codegen::ir::{StackSlotData, StackSlotKind};
use std::mem::{align_of, size_of};

impl CodegenCtx<'_> {
    pub(super) fn call(&mut self, expr_id: ExprId, call: &Call, builder: &mut FunctionBuilder<'_>) {
        match call.function() {
            "dbsp.error.abort" => self.error_abort(builder),

            // fn(*mut {vec_ptr, row_vtable}, row_value)
            "dbsp.row.vec.push" => self.row_vec_push(call, builder),

            // `fn(string: str, len: usize)` (mutates the given string)
            "dbsp.str.truncate" => self.string_truncate(call, builder),

            // `fn(string: str, len: usize) -> str`
            "dbsp.str.truncate_clone" => self.string_truncate_clone(expr_id, call, builder),

            // `fn(first: str, second: str)` (mutates the first string)
            "dbsp.str.concat" => self.string_concat(expr_id, call, builder),

            // `fn(first: str, second: str) -> str`
            "dbsp.str.concat_clone" => self.string_concat_clone(expr_id, call, builder),

            // `fn(string: str)` (mutates the given string)
            "dbsp.str.clear" => self.string_clear(call, builder),

            "dbsp.str.bit_length" => self.string_bit_length(expr_id, call, builder),
            "dbsp.str.char_length" => self.string_char_length(expr_id, call, builder),
            "dbsp.str.byte_length" => self.string_byte_length(expr_id, call, builder),

            "dbsp.str.write" => self.string_write(expr_id, call, builder),

            "dbsp.str.parse" => self.string_parse(expr_id, call, builder),

            function @ ("dbsp.str.is_nfc"
            | "dbsp.str.is_nfd"
            | "dbsp.str.is_nfkc"
            | "dbsp.str.is_nfkd"
            | "dbsp.str.is_lowercase"
            | "dbsp.str.is_uppercase") => self.string_checks(function, expr_id, call, builder),

            // `fn(timestamp) -> date
            "dbsp.timestamp.to_date" => self.timestamp_to_date(expr_id, call, builder),

            // `fn(timestamp) -> i64`
            "dbsp.timestamp.epoch" => self.timestamp_epoch(expr_id, call, builder),

            // `fn(timestamp) -> i64`
            "dbsp.timestamp.year" => {
                let millis = self.value(call.args()[0]);
                let year = self.timestamp_year(millis, builder);
                self.add_expr(expr_id, year, ColumnType::I64, None);
            }

            // TODO: Implement these all natively, they're all simple functions
            // and function dispatch is a heavy price (this is especially important
            // for frequently-used functions like `@dbsp.timestamp.year()`)
            "dbsp.timestamp.month" => self.timestamp_month(expr_id, call, builder),
            "dbsp.timestamp.day" => self.timestamp_day(expr_id, call, builder),
            "dbsp.timestamp.quarter" => self.timestamp_quarter(expr_id, call, builder),
            "dbsp.timestamp.decade" => self.timestamp_decade(expr_id, call, builder),
            "dbsp.timestamp.century" => self.timestamp_century(expr_id, call, builder),
            "dbsp.timestamp.millennium" => self.timestamp_millennium(expr_id, call, builder),
            "dbsp.timestamp.iso_year" => self.timestamp_iso_year(expr_id, call, builder),
            "dbsp.timestamp.week" => self.timestamp_week(expr_id, call, builder),
            "dbsp.timestamp.day_of_week" => self.timestamp_day_of_week(expr_id, call, builder),
            "dbsp.timestamp.iso_day_of_week" => {
                self.timestamp_iso_day_of_week(expr_id, call, builder);
            }
            "dbsp.timestamp.day_of_year" => self.timestamp_day_of_year(expr_id, call, builder),
            "dbsp.timestamp.millisecond" => self.timestamp_millisecond(expr_id, call, builder),
            "dbsp.timestamp.microsecond" => self.timestamp_microsecond(expr_id, call, builder),
            "dbsp.timestamp.second" => self.timestamp_second(expr_id, call, builder),
            "dbsp.timestamp.minute" => self.timestamp_minute(expr_id, call, builder),
            "dbsp.timestamp.hour" => self.timestamp_hour(expr_id, call, builder),
            "dbsp.timestamp.floor_week" => self.timestamp_floor_week(expr_id, call, builder),

            // `fn(date) -> timestamp
            "dbsp.date.to_timestamp" => self.date_to_timestamp(expr_id, call, builder),

            // `fn(date) -> i32`
            "dbsp.date.epoch" => self.date_epoch(expr_id, call, builder),

            // TODO: Implement these all natively, they're all simple functions
            // and function dispatch is a heavy price (this is especially important
            // for frequently-used functions like `@dbsp.date.year()`)
            "dbsp.date.year" => self.date_year(expr_id, call, builder),
            "dbsp.date.month" => self.date_month(expr_id, call, builder),
            "dbsp.date.day" => self.date_day(expr_id, call, builder),
            "dbsp.date.quarter" => self.date_quarter(expr_id, call, builder),
            "dbsp.date.decade" => self.date_decade(expr_id, call, builder),
            "dbsp.date.century" => self.date_century(expr_id, call, builder),
            "dbsp.date.millennium" => self.date_millennium(expr_id, call, builder),
            "dbsp.date.iso_year" => self.date_iso_year(expr_id, call, builder),
            "dbsp.date.week" => self.date_week(expr_id, call, builder),
            "dbsp.date.day_of_week" => self.date_day_of_week(expr_id, call, builder),
            "dbsp.date.iso_day_of_week" => self.date_iso_day_of_week(expr_id, call, builder),
            "dbsp.date.day_of_year" => self.date_day_of_year(expr_id, call, builder),

            // Date functions that always return zero
            function @ ("dbsp.date.hour"
            | "dbsp.date.minute"
            | "dbsp.date.second"
            | "dbsp.date.millisecond"
            | "dbsp.date.microsecond") => {
                self.constant_zero_date_function(expr_id, call, function, builder);
            }

            "dbsp.math.is_power_of_two" => self.math_is_power_of_two(expr_id, call, builder),
            "dbsp.math.is_sign_positive" => self.math_is_is_sign_positive(expr_id, call, builder),
            "dbsp.math.is_sign_negative" => self.math_is_is_sign_negative(expr_id, call, builder),

            trig if TRIG_INTRINSICS.contains(&trig) => {
                self.trig_intrinsic(expr_id, call, trig, builder)
            }
            "dbsp.math.cot" => self.math_cot(expr_id, call, builder),
            "dbsp.math.fdim" => self.math_fdim(expr_id, call, builder),
            "dbsp.math.radians_to_degrees" => self.math_radians_to_degrees(expr_id, call, builder),
            "dbsp.math.degrees_to_radians" => self.math_degrees_to_radians(expr_id, call, builder),

            unknown => todo!("unknown function call: @{unknown}"),
        }
    }

    fn error_abort(&self, builder: &mut FunctionBuilder<'_>) {
        let trap = builder.ins().trap(TRAP_ABORT);

        if let Some(writer) = self.comment_writer.as_deref() {
            writer
                .borrow_mut()
                .add_comment(trap, "call @dbsp.error.abort()");
        }
    }

    fn row_vec_push(&mut self, call: &Call, builder: &mut FunctionBuilder<'_>) {
        let vec_layout = self.layout_cache.row_layout_cache().row_vector();
        let native_vec_layout = self.layout_cache.layout_of(vec_layout);

        debug_assert_eq!(call.args().len(), 2);
        debug_assert_eq!(call.arg_types().len(), 2);
        debug_assert_eq!(call.arg_types()[0].as_row(), Some(vec_layout));
        let row_layout = self.expr_layouts[&call.args()[1]];
        debug_assert_eq!(call.arg_types()[1].as_row(), Some(row_layout));

        let (vec, row) = (self.value(call.args()[0]), self.value(call.args()[1]));

        let ptr_ty = self.pointer_type();
        let flags = MemFlags::trusted();
        let vec_ptr = builder
            .ins()
            .load(ptr_ty, flags, vec, native_vec_layout.offset_of(0) as i32);
        let vtable_ptr =
            builder
                .ins()
                .load(ptr_ty, flags, vec, native_vec_layout.offset_of(1) as i32);

        if self.debug_assertions() {
            // Assert that the vec pointer is valid
            self.assert_ptr_valid(vec_ptr, align_of::<Vec<crate::row::Row>>() as u32, builder);

            // Assert that the vtable pointer is valid
            self.assert_ptr_valid(vtable_ptr, align_of::<VTable>() as u32, builder);

            // Assert that the row pointer is valid
            let row_align = self.layout_cache.layout_of(row_layout).align();
            self.assert_ptr_valid(row, row_align, builder);

            // Assert that the vtable is associated with the correct layout
            let vtable_layout = builder.ins().load(
                types::I32,
                MemFlags::trusted(),
                vtable_ptr,
                VTable::layout_id_offset() as i32,
            );
            let expected_layout = builder
                .ins()
                .iconst(types::I32, row_layout.into_inner() as i64);
            let are_equal = builder
                .ins()
                .icmp(IntCC::Equal, vtable_layout, expected_layout);
            builder.ins().trapz(are_equal, TRAP_ASSERT_EQ);
        }

        let dbsp_row_vec_push = self.imports.get("row_vec_push", self.module, builder.func);
        let call_inst = builder
            .ins()
            .call(dbsp_row_vec_push, &[vec_ptr, vtable_ptr, row]);

        if let Some(writer) = self.comment_writer.as_deref() {
            let layout = self.layout_cache.layout_of(row_layout);
            writer
                .borrow_mut()
                .add_comment(call_inst, format!("call @dbsp.row.vec.push() for {layout}"));
        }
    }

    fn string_truncate(&mut self, call: &Call, builder: &mut FunctionBuilder<'_>) {
        let [string_id, length_id]: [_; 2] = call.args().try_into().unwrap();
        let (string, truncated_length) = (self.value(string_id), self.value(length_id));
        debug_assert!(!self.is_readonly(string_id));

        // Right now with the current design of strings (having them be a
        // pointer to { length, capacity, ..data } instead of { length, capacity, *data
        // }) we have to branch when setting the string's length since `ThinStr`
        // has a sigil value for empty strings that we can't mutate
        let after_block = builder.create_block();
        let set_string_length = builder.create_block();

        // If `string` is the sigil string, skip setting the string's length
        // (the sigil string's length of zero will always be less than or
        // equal to `truncated_length` so the behavior is identical, we
        // just can't mutate the sigil string)
        // TODO: Should we branch on `length == 0` instead of `string == sigil`?
        let is_sigil_string =
            builder
                .ins()
                .icmp_imm(IntCC::Equal, string, ThinStr::sigil_addr() as i64);
        builder
            .ins()
            .brif(is_sigil_string, after_block, &[], set_string_length, &[]);

        if let Some(writer) = self.comment_writer.as_deref() {
            let inst = builder.value_def(is_sigil_string);
            writer.borrow_mut().add_comment(
                inst,
                format!("call @dbsp.str.truncate({string}, {truncated_length})"),
            );
        }

        // Build the block where we set the string's truncated length
        builder.switch_to_block(set_string_length);

        if self.debug_assertions() {
            // Assert that the string pointer is a valid pointer
            self.assert_ptr_valid(string, ThinStr::ALIGN as u32, builder);

            // Assert that `truncated_length` is less than or equal to `isize::MAX`
            let less_than_max = builder.ins().icmp_imm(
                IntCC::UnsignedLessThanOrEqual,
                truncated_length,
                isize::MAX as i64,
            );
            builder.ins().trapz(less_than_max, TRAP_ASSERT_EQ);
        }

        // Get the current length of the string, will never be readonly since we're
        // about to manipulate it
        let current_length = self.string_length(string, false, builder);

        // The string's length will be set to the minimum of the proposed
        // length and the string's current length
        let new_length = builder.ins().umin(truncated_length, current_length);

        // Ensure that the chosen length is less than the string's capacity
        if self.debug_assertions() {
            // Will never be readonly since we're about to manipulate it
            let capacity = self.string_capacity(string, false, builder);
            let length_less_than_capacity =
                builder
                    .ins()
                    .icmp(IntCC::UnsignedLessThanOrEqual, new_length, capacity);

            builder
                .ins()
                .trapz(length_less_than_capacity, TRAP_ASSERT_EQ);
        }

        // Get the offset of the length field
        let length_offset = ThinStr::length_offset();

        // Store the new length to the length field
        builder.ins().store(
            MemFlags::trusted(),
            new_length,
            string,
            length_offset as i32,
        );

        // Jump & switch to the after block for the following code
        builder.ins().jump(after_block, &[]);
        builder.switch_to_block(after_block);
        builder.seal_block(set_string_length);
    }

    fn string_truncate_clone(
        &mut self,
        expr_id: ExprId,
        call: &Call,
        builder: &mut FunctionBuilder<'_>,
    ) {
        let [string_id, length_id]: [_; 2] = call.args().try_into().unwrap();
        let (string, truncated_length) = (self.value(string_id), self.value(length_id));
        // This string can be readonly since the function clones and doesn't manipulate
        // the underlying string
        let string_readonly = self.is_readonly(string_id);

        // Right now with the current design of strings (having them be a
        // pointer to { length, capacity, ..data } instead of { length, capacity, *data
        // }) we have to branch when setting the string's length since `ThinStr`
        // has a sigil value for empty strings that we can't mutate
        let after_block = builder.create_block();
        builder.append_block_param(after_block, self.pointer_type());

        let allocate_truncated_string = builder.create_block();

        // Get the current length of the string
        let current_length = self.string_length(string, string_readonly, builder);
        // The string's length will be set to the minimum of the proposed
        // length and the string's current length
        let new_length = builder.ins().umin(truncated_length, current_length);

        // If `new_length` is zero, we can directly return an empty string
        let new_length_is_zero = builder.ins().icmp_imm(IntCC::Equal, new_length, 0);

        // If `string` is the sigil string, skip setting the string's length
        // (the sigil string's length of zero will always be less than or
        // equal to `truncated_length` so the behavior is identical, we
        // just can't mutate the sigil string)
        // TODO: Should we branch on `length == 0` instead of `string == sigil`?
        let empty_string = builder
            .ins()
            .iconst(self.pointer_type(), ThinStr::sigil_addr() as i64);
        let is_sigil_string = builder.ins().icmp(IntCC::Equal, string, empty_string);

        let should_return_empty = builder.ins().bor(new_length_is_zero, is_sigil_string);
        builder.ins().brif(
            should_return_empty,
            // If we skip allocating a truncated string, we can directly return an empty string
            after_block,
            &[empty_string],
            allocate_truncated_string,
            &[],
        );

        if let Some(writer) = self.comment_writer.as_deref() {
            let inst = builder.value_def(is_sigil_string);
            writer.borrow_mut().add_comment(
                inst,
                format!("call @dbsp.str.truncate({string}, {truncated_length})"),
            );
        }

        // Build the block where we set the string's truncated length
        builder.switch_to_block(allocate_truncated_string);

        // Allocate a string with the given capacity
        let string_with_capacity =
            self.imports
                .get("string_with_capacity", self.module, builder.func);
        let allocated = builder.call_fn(string_with_capacity, &[new_length]);

        // Ensure that neither `string` nor `allocated` are empty strings
        if self.debug_assertions() {
            let string_is_empty = builder.ins().icmp(IntCC::Equal, string, empty_string);
            builder.ins().trapnz(string_is_empty, TRAP_ASSERT_EQ);

            let allocated_is_empty = builder.ins().icmp(IntCC::Equal, allocated, empty_string);
            builder.ins().trapnz(allocated_is_empty, TRAP_ASSERT_EQ);
        }

        // Copy over the data into the allocated string
        let allocated_ptr = self.string_ptr(allocated, builder);
        builder.call_memmove(self.frontend_config(), allocated_ptr, string, new_length);

        // Get the offset of the length field
        let length_offset = ThinStr::length_offset();

        // Store the new length to the length field
        builder.ins().store(
            MemFlags::trusted(),
            new_length,
            allocated,
            length_offset as i32,
        );

        // Jump & switch to the after block for the following code
        builder.ins().jump(after_block, &[allocated]);
        builder.switch_to_block(after_block);
        builder.seal_block(allocate_truncated_string);

        // Get the cloned string
        let cloned = builder.block_params(after_block)[0];
        self.add_expr(expr_id, cloned, ColumnType::String, None);
    }

    fn string_clear(&mut self, call: &Call, builder: &mut FunctionBuilder<'_>) {
        let string_id = call.args()[0];
        let string = self.value(string_id);
        debug_assert!(!self.is_readonly(string_id));

        // Right now with the current design of strings (having them be a
        // pointer to { length, capacity, ..data } instead of { length, capacity, *data
        // }) we have to branch when setting the string's length since `ThinStr`
        // has a sigil value for empty strings that we can't mutate
        let after_block = builder.create_block();
        let set_string_length = builder.create_block();

        // If `string` is the sigil string, skip setting the string's length
        // since we can't mutate the sigil string and its length is already zero
        let string_capacity = builder.ins().load(
            self.pointer_type(),
            MemFlags::trusted(),
            string,
            ThinStr::capacity_offset() as i32,
        );
        let is_capacity_zero = builder.ins().icmp_imm(IntCC::Equal, string_capacity, 0);

        builder
            .ins()
            .brif(is_capacity_zero, after_block, &[], set_string_length, &[]);

        if let Some(writer) = self.comment_writer.as_deref() {
            let inst = builder.value_def(string_capacity);
            writer
                .borrow_mut()
                .add_comment(inst, format!("call @dbsp.str.clear({string})"));
        }

        // Build the block where we set the string's length to zero
        builder.switch_to_block(set_string_length);

        // Assert that the string pointer is a valid pointer
        self.debug_assert_ptr_valid(string, ThinStr::ALIGN as u32, builder);

        // The string's length will be set to zero
        let new_length = builder.ins().iconst(self.pointer_type(), 0);

        // Get the offset of the length field
        let length_offset = ThinStr::length_offset();

        // Store the new length to the length field
        builder.ins().store(
            MemFlags::trusted(),
            new_length,
            string,
            length_offset as i32,
        );

        // Jump & switch to the after block for the following code
        builder.ins().jump(after_block, &[]);
        builder.switch_to_block(after_block);
        builder.seal_block(set_string_length);
    }

    // TODO: Should we provide a few more specializations other than 2?
    fn string_concat(&mut self, expr_id: ExprId, call: &Call, builder: &mut FunctionBuilder<'_>) {
        // Concatenation for two strings
        if call.args().len() == 2 {
            let [target_id, string_id]: [_; 2] = call.args().try_into().unwrap();
            let (target, string) = (self.value(target_id), self.value(string_id));

            let (string_ptr, string_len) = (
                self.string_ptr(string, builder),
                self.string_length(string, self.is_readonly(string_id), builder),
            );

            let push_str = self
                .imports
                .get("string_push_str", self.module, builder.func);
            let concatenated = builder.call_fn(push_str, &[target, string_ptr, string_len]);

            self.add_expr(expr_id, concatenated, ColumnType::String, None);
            if let Some(writer) = self.comment_writer.as_deref() {
                writer.borrow_mut().add_comment(
                    builder.value_def(concatenated),
                    format!("call @dbsp.str.concat({target}, {string})"),
                );
            }

        // Concatenation for an unbounded number of strings
        } else {
            let target_id = call.args()[0];
            let target = self.value(target_id);

            let strings = &call.args()[1..];

            // Make a stack slot to hold the strings we're passing
            let slice_size = size_of::<*const u8>() * 2;
            let slot_size = strings.len() * slice_size;
            let string_slot = builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                slot_size as u32,
            ));

            // Fill the stack slot with all of the strings
            for (idx, &string_id) in strings.iter().enumerate() {
                let string = self.value(string_id);
                let (string_ptr, string_len) = (
                    self.string_ptr(string, builder),
                    self.string_length(string, self.is_readonly(string_id), builder),
                );

                // Store each string's pointer and length to the stack slot
                builder
                    .ins()
                    .stack_store(string_ptr, string_slot, (idx * slice_size) as i32);
                builder.ins().stack_store(
                    string_len,
                    string_slot,
                    (idx * slice_size + size_of::<*const u8>()) as i32,
                );
            }

            // Get the address and length of the strings
            let ptr_ty = self.pointer_type();
            let strings_addr = builder.ins().stack_addr(ptr_ty, string_slot, 0);
            let strings_len = builder.ins().iconst(ptr_ty, strings.len() as i64);

            // Call the variadic push function
            let push_str = self
                .imports
                .get("string_push_str_variadic", self.module, builder.func);
            let concatenated = builder.call_fn(push_str, &[target, strings_addr, strings_len]);

            self.add_expr(expr_id, concatenated, ColumnType::String, None);
            if let Some(writer) = self.comment_writer.as_deref() {
                writer.borrow_mut().add_comment(
                    builder.value_def(concatenated),
                    format!("call @dbsp.str.concat({target_id}, {strings:?})"),
                );
            }
        }
    }

    fn string_concat_clone(
        &mut self,
        expr_id: ExprId,
        call: &Call,
        builder: &mut FunctionBuilder<'_>,
    ) {
        let string_ids = call.args();
        let strings: Vec<_> = string_ids
            .iter()
            .map(|&string| self.value(string))
            .collect();

        // Load the string's lengths
        let lengths: Vec<_> = strings
            .iter()
            .zip(string_ids)
            .map(|(&string, &string_id)| {
                self.string_length(string, self.is_readonly(string_id), builder)
            })
            .collect();

        // TODO: A tree reduction is probably better here
        // TODO: `.uadd_overflow_trap(sum, length, TRAP_CAPACITY_OVERFLOW)`?
        let total_length = lengths[1..]
            .iter()
            .fold(lengths[0], |sum, &length| builder.ins().iadd(sum, length));

        let do_concat = builder.create_block();
        let after = builder.create_block();
        builder.append_block_param(after, self.pointer_type());

        let sigil = builder
            .ins()
            .iconst(self.pointer_type(), ThinStr::sigil_addr() as i64);
        builder
            .ins()
            .brif(total_length, do_concat, &[], after, &[sigil]);

        builder.switch_to_block(do_concat);

        // Allocate a string of the requested capacity
        let with_capacity = self
            .imports
            .get("string_with_capacity", self.module, builder.func);
        let allocated = builder.call_fn(with_capacity, &[total_length]);

        // Copy each string into the allocated string
        let mut target_pointer = self.string_ptr(allocated, builder);
        for (idx, (string, length)) in strings.into_iter().zip(lengths).enumerate() {
            // Copy each string into the allocation
            let string_ptr = self.string_ptr(string, builder);
            builder.call_memmove(self.frontend_config(), target_pointer, string_ptr, length);

            if idx + 1 != string_ids.len() {
                // Advance the pointer
                target_pointer = builder.ins().iadd(target_pointer, length);
            }
        }

        // Set the allocated string's length
        let length_offset = ThinStr::length_offset();
        builder.ins().store(
            MemFlags::trusted(),
            total_length,
            allocated,
            length_offset as i32,
        );

        builder.ins().jump(after, &[allocated]);
        builder.switch_to_block(after);

        let concatenated_string = builder.block_params(after)[0];
        self.add_expr(expr_id, concatenated_string, ColumnType::String, None);

        if let Some(writer) = self.comment_writer.as_deref() {
            let inst = builder.value_def(allocated);
            writer
                .borrow_mut()
                .add_comment(inst, format!("call @dbsp.str.concat_clone({string_ids:?})"));
        }
    }

    fn string_write(&mut self, expr_id: ExprId, call: &Call, builder: &mut FunctionBuilder<'_>) {
        let [target_id, value_id]: [_; 2] = call.args().try_into().unwrap();
        let (target, value) = (self.value(target_id), self.value(value_id));

        let written = match self.expr_ty(value_id) {
            ty @ (ColumnType::U8
            | ColumnType::I8
            | ColumnType::U16
            | ColumnType::I16
            | ColumnType::U32
            | ColumnType::I32
            | ColumnType::U64
            | ColumnType::I64
            | ColumnType::Usize
            | ColumnType::Isize
            | ColumnType::F32
            | ColumnType::F64
            | ColumnType::Date
            | ColumnType::Timestamp) => {
                let intrinsic = match ty {
                    ColumnType::U8 => "write_i8_to_string",
                    ColumnType::I8 => "write_u8_to_string",
                    ColumnType::U16 => "write_u16_to_string",
                    ColumnType::I16 => "write_i16_to_string",
                    ColumnType::U32 => "write_u32_to_string",
                    ColumnType::I32 => "write_i32_to_string",
                    ColumnType::U64 => "write_u64_to_string",
                    ColumnType::I64 => "write_i64_to_string",
                    ColumnType::F32 => "write_f32_to_string",
                    ColumnType::F64 => "write_f64_to_string",
                    ColumnType::Usize => {
                        if self.pointer_type().bits() == 32 {
                            "write_u32_to_string"
                        } else {
                            "write_u64_to_string"
                        }
                    }
                    ColumnType::Isize => {
                        if self.pointer_type().bits() == 32 {
                            "write_i32_to_string"
                        } else {
                            "write_i64_to_string"
                        }
                    }
                    ColumnType::Date => "write_date_to_string",
                    ColumnType::Timestamp => "write_timestamp_to_string",

                    ColumnType::Bool | ColumnType::String | ColumnType::Unit | ColumnType::Ptr => {
                        unreachable!()
                    }
                };

                let write = self.imports.get(intrinsic, self.module, builder.func);
                builder.call_fn(write, &[target, value])
            }

            // Write a boolean to the string
            ColumnType::Bool => {
                let (true_ptr, true_len) = self.import_string("true", builder);
                let (false_ptr, false_len) = self.import_string("false", builder);

                let (bool_ptr, bool_len) = (
                    builder.ins().select(value, true_ptr, false_ptr),
                    builder.ins().select(value, true_len, false_len),
                );

                let push_str = self
                    .imports
                    .get("string_push_str", self.module, builder.func);
                builder.call_fn(push_str, &[target, bool_ptr, bool_len])
            }

            // Write `unit` to the string
            ColumnType::Unit => {
                let (unit_ptr, unit_len) = self.import_string("unit", builder);

                let push_str = self
                    .imports
                    .get("string_push_str", self.module, builder.func);
                builder.call_fn(push_str, &[target, unit_ptr, unit_len])
            }

            // Push the string to the target string
            ColumnType::String => {
                let (string_ptr, string_len) = (
                    self.string_ptr(value, builder),
                    self.string_length(value, self.is_readonly(value_id), builder),
                );

                let push_str = self
                    .imports
                    .get("string_push_str", self.module, builder.func);
                builder.call_fn(push_str, &[target, string_ptr, string_len])
            }

            ColumnType::Ptr => unreachable!(),
        };

        self.add_expr(expr_id, written, ColumnType::String, None);
        if let Some(writer) = self.comment_writer.as_deref() {
            writer.borrow_mut().add_comment(
                builder.value_def(written),
                format!("call @dbsp.str.write({target_id}, {value_id})"),
            );
        }
    }

    fn string_bit_length(
        &mut self,
        expr_id: ExprId,
        call: &Call,
        builder: &mut FunctionBuilder<'_>,
    ) {
        let string_id = call.args()[0];
        let string = self.value(string_id);

        let length = self.string_length(string, self.is_readonly(string_id), builder);
        let bits = builder.ins().imul_imm(length, 8);
        self.add_expr(expr_id, bits, ColumnType::Usize, None);
    }

    fn string_char_length(
        &mut self,
        expr_id: ExprId,
        call: &Call,
        builder: &mut FunctionBuilder<'_>,
    ) {
        let string_id = call.args()[0];
        let string = self.value(string_id);

        let ptr = self.string_ptr(string, builder);
        let length = self.string_length(string, self.is_readonly(string_id), builder);

        let count_chars = self
            .imports
            .get("string_count_chars", self.module, builder.func);
        let chars = builder.call_fn(count_chars, &[ptr, length]);

        self.add_expr(expr_id, chars, ColumnType::Usize, None);
    }

    fn string_byte_length(
        &mut self,
        expr_id: ExprId,
        call: &Call,
        builder: &mut FunctionBuilder<'_>,
    ) {
        let string_id = call.args()[0];
        let string = self.value(string_id);

        let length = self.string_length(string, self.is_readonly(string_id), builder);
        self.add_expr(expr_id, length, ColumnType::Usize, None);
    }

    fn string_checks(
        &mut self,
        func: &str,
        expr_id: ExprId,
        call: &Call,
        builder: &mut FunctionBuilder<'_>,
    ) {
        let intrinsic = match func {
            "dbsp.str.is_nfc" => "string_is_nfc",
            "dbsp.str.is_nfd" => "string_is_nfd",
            "dbsp.str.is_nfkc" => "string_is_nfkc",
            "dbsp.str.is_nfkd" => "string_is_nfkd",
            "dbsp.str.is_lowercase" => "string_is_lowercase",
            "dbsp.str.is_uppercase" => "string_is_uppercase",
            // TODO: is_ascii can be implemented manually
            "dbsp.str.is_ascii" => "string_is_ascii",
            _ => unreachable!(),
        };

        let string_id = call.args()[0];
        let string = self.value(string_id);

        let ptr = self.string_ptr(string, builder);
        let length = self.string_length(string, self.is_readonly(string_id), builder);

        let string_is = self.imports.get(intrinsic, self.module, builder.func);
        let string_is = builder.call_fn(string_is, &[ptr, length]);

        self.add_expr(expr_id, string_is, ColumnType::Bool, None);

        if let Some(writer) = self.comment_writer.as_deref() {
            let inst = builder.value_def(string_is);
            writer
                .borrow_mut()
                .add_comment(inst, format!("call @{func}({string})"));
        }
    }

    fn timestamp_epoch(&mut self, expr_id: ExprId, call: &Call, builder: &mut FunctionBuilder<'_>) {
        let timestamp = self.value(call.args()[0]);

        // timestamp.milliseconds() / 1000
        let epoch = builder.ins().sdiv_imm(timestamp, 1000);
        self.add_expr(expr_id, epoch, ColumnType::I64, None);

        if let Some(writer) = self.comment_writer.as_deref() {
            let inst = builder.value_def(epoch);
            writer
                .borrow_mut()
                .add_comment(inst, format!("call @dbsp.timestamp.epoch({timestamp})"));
        }
    }

    fn timestamp_floor_week(
        &mut self,
        expr_id: ExprId,
        call: &Call,
        builder: &mut FunctionBuilder<'_>,
    ) {
        let timestamp = self.value(call.args()[0]);

        let intrinsic = self
            .imports
            .get("timestamp_floor_week", self.module, builder.func);
        let value = builder.call_fn(intrinsic, &[timestamp]);
        self.add_expr(expr_id, value, ColumnType::Timestamp, None);

        if let Some(writer) = self.comment_writer.as_deref() {
            let inst = builder.value_def(value);
            writer.borrow_mut().add_comment(
                inst,
                format!("call @dbsp.timestamp.floor_week({timestamp})"),
            );
        }
    }

    fn timestamp_to_date(
        &mut self,
        expr_id: ExprId,
        call: &Call,
        builder: &mut FunctionBuilder<'_>,
    ) {
        let timestamp = self.value(call.args()[0]);

        // (timestamp.milliseconds() / (86400 * 1000)) as i32
        let days = builder.ins().sdiv_imm(timestamp, 86400 * 1000);
        let days_i32 = builder.ins().ireduce(types::I32, days);
        self.add_expr(expr_id, days_i32, ColumnType::I32, None);

        if let Some(writer) = self.comment_writer.as_deref() {
            let inst = builder.value_def(days);
            writer
                .borrow_mut()
                .add_comment(inst, format!("call @dbsp.timestamp.to_date({timestamp})"));
        }
    }

    fn date_to_timestamp(
        &mut self,
        expr_id: ExprId,
        call: &Call,
        builder: &mut FunctionBuilder<'_>,
    ) {
        let date = self.value(call.args()[0]);

        // date.days() as i64 * 86400 * 1000
        let days = builder.ins().uextend(types::I64, date);
        let milliseconds = builder.ins().imul_imm(days, 86400 * 1000);
        self.add_expr(expr_id, milliseconds, ColumnType::Timestamp, None);

        if let Some(writer) = self.comment_writer.as_deref() {
            let inst = builder.value_def(days);
            writer
                .borrow_mut()
                .add_comment(inst, format!("call @dbsp.date.to_timestamp({date})"));
        }
    }

    fn date_epoch(&mut self, expr_id: ExprId, call: &Call, builder: &mut FunctionBuilder<'_>) {
        let date = self.value(call.args()[0]);

        // date.days() as i64 * 86400
        // TODO: The real code uses an i64 but we keep it as an i32 here
        // let date = builder.ins().uextend(types::I64, date);
        let epoch = builder.ins().imul_imm(date, 86400);
        self.add_expr(expr_id, epoch, ColumnType::I32, None);

        if let Some(writer) = self.comment_writer.as_deref() {
            let inst = builder.value_def(epoch);
            writer
                .borrow_mut()
                .add_comment(inst, format!("call @dbsp.date.epoch({date})"));
        }
    }

    fn constant_zero_date_function(
        &mut self,
        expr_id: ExprId,
        call: &Call,
        function: &str,
        builder: &mut FunctionBuilder<'_>,
    ) {
        let zero = builder.ins().iconst(types::I32, 0);
        self.add_expr(expr_id, zero, ColumnType::I32, None);

        if let Some(writer) = self.comment_writer.as_deref() {
            let inst = builder.value_def(zero);
            writer.borrow_mut().add_comment(
                inst,
                format!("call @{function}({})", self.value(call.args()[0])),
            );
        }
    }

    fn math_is_power_of_two(
        &mut self,
        expr_id: ExprId,
        call: &Call,
        builder: &mut FunctionBuilder<'_>,
    ) {
        let int = self.value(call.args()[0]);
        debug_assert!(builder.value_type(int).is_int());

        // `(x.count_ones() == 1) ≡ (2ⁿ == x)`
        let ones = builder.ins().popcnt(int);
        let is_power_of_two = builder.ins().icmp_imm(IntCC::Equal, ones, 1);
        self.add_expr(expr_id, is_power_of_two, ColumnType::Bool, None);

        if let Some(writer) = self.comment_writer.as_deref() {
            let inst = builder.value_def(ones);
            writer
                .borrow_mut()
                .add_comment(inst, format!("call @dbsp.math.is_power_of_two({int})"));
        }
    }

    fn math_cot(&mut self, expr_id: ExprId, call: &Call, builder: &mut FunctionBuilder<'_>) {
        let float = self.value(call.args()[0]);
        let float_ty = builder.value_type(float);
        debug_assert!(float_ty.is_float());

        let tan = if float_ty == types::F32 {
            "tanf"
        } else {
            debug_assert_eq!(float_ty, types::F64);
            "tan"
        };
        let tan = self.imports.get(tan, self.module, builder.func);
        let tan = builder.call_fn(tan, &[float]);

        // 1.0 / tan(x)
        let one = builder.float_one(float_ty);
        let cotan = builder.ins().fdiv(one, tan);

        self.add_expr(
            expr_id,
            cotan,
            call.arg_types()[0].as_scalar().unwrap(),
            None,
        );
    }

    // https://github.com/rust-lang/libm/blob/master/src/math/fdim.rs
    // https://github.com/rust-lang/libm/blob/master/src/math/fdimf.rs
    //
    // ```
    // if x.is_nan() {
    //     x
    // } else if y.is_nan() {
    //     y
    // } else if x > y {
    //     x - y
    // } else {
    //     0.0
    // }
    // ```
    fn math_fdim(&mut self, expr_id: ExprId, call: &Call, builder: &mut FunctionBuilder<'_>) {
        let (x, y) = (self.value(call.args()[0]), self.value(call.args()[1]));
        let float_ty = builder.value_type(x);
        debug_assert!(float_ty.is_float());
        debug_assert_eq!(float_ty, builder.value_type(y));

        let x_nan = self.is_nan(x, builder);
        let y_nan = self.is_nan(x, builder);
        let nan_value = builder.ins().select(x_nan, x, y);
        let either_nan = builder.ins().bor(x_nan, y_nan);

        let zero = builder.float_zero(float_ty);
        let x_sub_y = builder.ins().fsub(x, y);
        let x_gt_y = builder.ins().fcmp(FloatCC::GreaterThan, x, y);
        let non_nan_value = builder.ins().select(x_gt_y, x_sub_y, zero);

        let fdim = builder.ins().select(either_nan, nan_value, non_nan_value);

        self.add_expr(
            expr_id,
            fdim,
            call.arg_types()[0].as_scalar().unwrap(),
            None,
        );
    }

    // ```
    // fn to_degrees(self) -> f32 {
    //     // Use a constant for better precision.
    //     const PIS_IN_180: f32 = 57.2957795130823208767981548141051703_f32;
    //     self * PIS_IN_180
    // }
    //
    // fn to_degrees(self) -> f64 {
    //     // The division here is correctly rounded with respect to the true
    //     // value of 180/π. (This differs from f32, where a constant must be
    //     // used to ensure a correctly rounded result.)
    //     self * (180.0f64 / consts::PI)
    // }
    // ```
    fn math_radians_to_degrees(
        &mut self,
        expr_id: ExprId,
        call: &Call,
        builder: &mut FunctionBuilder<'_>,
    ) {
        let float = self.value(call.args()[0]);
        let float_ty = call.arg_types()[0].as_scalar().unwrap();
        debug_assert!(float_ty.is_float());

        #[allow(clippy::excessive_precision)]
        let pis_in_180 = if float_ty.is_f32() {
            builder
                .ins()
                .f32const(57.2957795130823208767981548141051703)
        } else {
            let one_eighty = builder.ins().f64const(180.0);
            let pi = builder.ins().f64const(core::f64::consts::PI);
            builder.ins().fdiv(one_eighty, pi)
        };

        let degrees = builder.ins().fmul(float, pis_in_180);

        self.add_expr(expr_id, degrees, float_ty, None);
    }

    // ```
    // fn to_radians(self) -> f32 {
    //     let value: f32 = consts::PI;
    //     self * (value / 180.0f32)
    // }
    //
    // fn to_radians(self) -> f64 {
    //     let value: f64 = consts::PI;
    //     self * (value / 180.0)
    // }
    // ```
    fn math_degrees_to_radians(
        &mut self,
        expr_id: ExprId,
        call: &Call,
        builder: &mut FunctionBuilder<'_>,
    ) {
        let float = self.value(call.args()[0]);
        let float_ty = builder.value_type(float);
        debug_assert!(float_ty.is_float());

        let pi = builder.float_pi(float_ty);
        let one_eighty = if float_ty == types::F32 {
            builder.ins().f32const(180.0)
        } else {
            builder.ins().f64const(180.0)
        };
        let pi_div_180 = builder.ins().fdiv(pi, one_eighty);

        let radians = builder.ins().fmul(float, pi_div_180);

        self.add_expr(
            expr_id,
            radians,
            call.arg_types()[0].as_scalar().unwrap(),
            None,
        );
    }

    fn math_is_is_sign_positive(
        &mut self,
        expr_id: ExprId,
        call: &Call,
        builder: &mut FunctionBuilder<'_>,
    ) {
        let value = self.value(call.args()[0]);
        let value_ty = call.arg_types()[0].as_scalar().unwrap();

        // Check if signed values are greater than or equal to zero
        let is_negative = if value_ty.is_signed_int() {
            builder
                .ins()
                .icmp_imm(IntCC::SignedGreaterThanOrEqual, value, 0)

        // Unsigned values are always positive
        } else if value_ty.is_unsigned_int() {
            builder.true_byte()

        // Transmute float to int and mask the sign bit
        } else if value_ty.is_float() {
            let int_ty = builder.value_type(value).as_int();

            let mask = if int_ty == types::I32 {
                0x8000_0000i64
            } else if int_ty == types::I64 {
                0x8000_0000_0000_0000u64 as i64
            } else {
                unreachable!()
            };

            let int_value = builder.ins().bitcast(int_ty, MemFlags::new(), value);
            let sign = builder.ins().band_imm(int_value, mask);
            builder.ins().icmp_imm(IntCC::Equal, sign, 0)

        // Should be prevented by validation
        } else {
            unreachable!(
                "invalid value for is_sign_negative: expected float or int, got {value_ty}",
            )
        };

        self.add_expr(expr_id, is_negative, ColumnType::Bool, None);
    }

    fn math_is_is_sign_negative(
        &mut self,
        expr_id: ExprId,
        call: &Call,
        builder: &mut FunctionBuilder<'_>,
    ) {
        let value = self.value(call.args()[0]);
        let value_ty = call.arg_types()[0].as_scalar().unwrap();

        // Check if signed values are less than zero
        let is_negative = if value_ty.is_signed_int() {
            builder.ins().icmp_imm(IntCC::SignedLessThan, value, 0)

        // Unsigned values are always positive
        } else if value_ty.is_unsigned_int() {
            builder.false_byte()

        // Transmute float to int and mask the sign bit
        } else if value_ty.is_float() {
            let int_ty = builder.value_type(value).as_int();

            let mask = if int_ty == types::I32 {
                0x8000_0000i64
            } else if int_ty == types::I64 {
                0x8000_0000_0000_0000u64 as i64
            } else {
                unreachable!()
            };

            let int_value = builder.ins().bitcast(int_ty, MemFlags::new(), value);
            let sign = builder.ins().band_imm(int_value, mask);
            builder.ins().icmp_imm(IntCC::NotEqual, sign, 0)

        // Should be prevented by validation
        } else {
            unreachable!(
                "invalid value for is_sign_negative: expected float or int, got {value_ty}",
            )
        };

        self.add_expr(expr_id, is_negative, ColumnType::Bool, None);
    }

    fn string_parse(&mut self, expr_id: ExprId, call: &Call, builder: &mut FunctionBuilder<'_>) {
        match call.ret_ty() {
            // TODO: Bool "parsing" can be inlined
            ColumnType::Bool => {
                self.parse_scalar_from_string("parse_bool_from_str", expr_id, call, builder);
            }

            // TODO: Integer parsing can be fairly easily inlined which also brings support
            // for parsing radix (radices?) other than base 10
            // See <https://github.com/Alexhuszagh/rust-lexical/blob/main/lexical-parse-integer/src/algorithm.rs>
            ColumnType::U8 => {
                self.parse_scalar_from_string("parse_u8_from_str", expr_id, call, builder);
            }
            ColumnType::I8 => {
                self.parse_scalar_from_string("parse_i8_from_str", expr_id, call, builder);
            }
            ColumnType::U16 => {
                self.parse_scalar_from_string("parse_u16_from_str", expr_id, call, builder);
            }
            ColumnType::I16 => {
                self.parse_scalar_from_string("parse_i16_from_str", expr_id, call, builder);
            }
            ColumnType::U32 => {
                self.parse_scalar_from_string("parse_u32_from_str", expr_id, call, builder);
            }
            ColumnType::I32 => {
                self.parse_scalar_from_string("parse_i32_from_str", expr_id, call, builder);
            }
            ColumnType::U64 => {
                self.parse_scalar_from_string("parse_u64_from_str", expr_id, call, builder);
            }
            ColumnType::I64 => {
                self.parse_scalar_from_string("parse_i64_from_str", expr_id, call, builder);
            }

            ColumnType::Usize => {
                if self.usize_is_u64() {
                    self.parse_scalar_from_string("parse_u64_from_str", expr_id, call, builder);
                } else {
                    self.parse_scalar_from_string("parse_u32_from_str", expr_id, call, builder);
                }
            }
            ColumnType::Isize => {
                if self.isize_is_i64() {
                    self.parse_scalar_from_string("parse_i64_from_str", expr_id, call, builder);
                } else {
                    self.parse_scalar_from_string("parse_i32_from_str", expr_id, call, builder);
                }
            }

            ColumnType::F32 => {
                self.parse_scalar_from_string("parse_f32_from_str", expr_id, call, builder);
            }
            ColumnType::F64 => {
                self.parse_scalar_from_string("parse_f64_from_str", expr_id, call, builder);
            }

            ColumnType::Date
            | ColumnType::Timestamp
            | ColumnType::String
            | ColumnType::Unit
            | ColumnType::Ptr => todo!(),
        }
    }

    fn parse_scalar_from_string(
        &mut self,
        intrinsic: &str,
        expr_id: ExprId,
        call: &Call,
        builder: &mut FunctionBuilder<'_>,
    ) {
        let string = self.value(call.args()[0]);
        let length = self.string_length(string, self.is_readonly(call.args()[0]), builder);
        let ptr = self.string_ptr(string, builder);

        let ret_ty = call.ret_ty().native_type().unwrap();

        let slot = builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            ret_ty.size(&self.frontend_config()),
        ));
        let slot_ptr = builder.ins().stack_addr(self.pointer_type(), slot, 0);

        let intrinsic = self.imports.get(intrinsic, self.module, builder.func);
        let errored = builder.call_fn(intrinsic, &[ptr, length, slot_ptr]);

        // FIXME: We should return a two-valued struct to allow users to handle
        // failed parsing, which is super expected
        builder.ins().trapnz(errored, TRAP_FAILED_PARSE);

        let value = builder
            .ins()
            .stack_load(ret_ty.native_type(&self.frontend_config()), slot, 0);
        self.add_expr(expr_id, value, Some(call.ret_ty()), None);
    }
}

macro_rules! timestamp_intrinsics {
    ($($name:ident),+ $(,)?) => {
        impl CodegenCtx<'_> {
            paste::paste! {
                $(
                    fn [<timestamp_ $name>](&mut self, expr_id: ExprId, call: &Call, builder: &mut FunctionBuilder<'_>) {
                        let timestamp = self.value(call.args()[0]);

                        let intrinsic = self
                            .imports
                            .get(stringify!([<timestamp_ $name>]), self.module, builder.func);
                        let value = builder.call_fn(intrinsic, &[timestamp]);
                        self.add_expr(expr_id, value, ColumnType::I64, None);

                        if let Some(writer) = self.comment_writer.as_deref() {
                            let inst = builder.value_def(value);
                            writer.borrow_mut().add_comment(
                                inst,
                                format!("call @dbsp.timestamp.{}({timestamp})", stringify!($name)),
                            );
                        }
                    }
                )+
            }
        }
    };
}

timestamp_intrinsics! {
    // year,
    month,
    day,
    quarter,
    decade,
    century,
    millennium,
    iso_year,
    week,
    day_of_week,
    iso_day_of_week,
    day_of_year,
    millisecond,
    microsecond,
    second,
    minute,
    hour,
}

macro_rules! date_intrinsics {
    ($($name:ident),+ $(,)?) => {
        impl CodegenCtx<'_> {
            paste::paste! {
                $(
                    fn [<date_ $name>](&mut self, expr_id: ExprId, call: &Call, builder: &mut FunctionBuilder<'_>) {
                        let date = self.value(call.args()[0]);

                        let intrinsic = self
                            .imports
                            .get(stringify!([<date_ $name>]), self.module, builder.func);
                        let value = builder.call_fn(intrinsic, &[date]);
                        self.add_expr(expr_id, value, ColumnType::I64, None);

                        if let Some(writer) = self.comment_writer.as_deref() {
                            let inst = builder.value_def(value);
                            writer.borrow_mut().add_comment(
                                inst,
                                format!("call @dbsp.date.{}({date})", stringify!($name)),
                            );
                        }
                    }
                )+
            }
        }
    };
}

date_intrinsics! {
    day,
    week,
    month,
    quarter,
    year,
    decade,
    century,
    millennium,
    iso_year,
    day_of_week,
    iso_day_of_week,
    day_of_year,
}
