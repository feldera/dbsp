/*
 * Copyright 2023 VMware, Inc.
 * SPDX-License-Identifier: MIT
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

package org.dbsp.sqlCompiler.compiler.backend.jit;

import org.dbsp.sqlCompiler.compiler.IErrorReporter;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.JITParameter;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.JITParameterMapping;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.cfg.JITBlock;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.cfg.JITBlockDestination;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.cfg.JITBranchTerminator;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.cfg.JITJumpTerminator;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.cfg.JITReturnTerminator;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.instructions.JITBinaryInstruction;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.instructions.JITCastInstruction;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.instructions.JITConstantInstruction;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.instructions.JITCopyInstruction;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.instructions.JITFunctionCall;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.instructions.JITInstruction;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.instructions.JITInstructionPair;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.instructions.JITInstructionRef;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.instructions.JITIsNullInstruction;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.instructions.JITLiteral;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.instructions.JITLoadInstruction;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.instructions.JITMuxInstruction;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.instructions.JITSetNullInstruction;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.instructions.JITStoreInstruction;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.instructions.JITUnaryInstruction;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.instructions.JITUninitInstruction;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.types.JITBoolType;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.types.JITDateType;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.types.JITI64Type;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.types.JITRowType;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.types.JITScalarType;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.types.JITStringType;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.types.JITTimestampType;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.types.JITType;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.types.JITUnitType;
import org.dbsp.sqlCompiler.compiler.errors.InternalCompilerError;
import org.dbsp.sqlCompiler.compiler.frontend.CalciteObject;
import org.dbsp.sqlCompiler.compiler.visitors.VisitDecision;
import org.dbsp.sqlCompiler.compiler.visitors.inner.InnerVisitor;
import org.dbsp.sqlCompiler.ir.expression.*;
import org.dbsp.sqlCompiler.ir.expression.literal.*;
import org.dbsp.sqlCompiler.ir.statement.DBSPLetStatement;
import org.dbsp.sqlCompiler.ir.statement.DBSPStatement;
import org.dbsp.sqlCompiler.ir.type.DBSPType;
import org.dbsp.sqlCompiler.ir.type.DBSPTypeTuple;
import org.dbsp.sqlCompiler.ir.type.DBSPTypeTupleBase;
import org.dbsp.sqlCompiler.ir.type.IsNumericType;
import org.dbsp.sqlCompiler.ir.type.primitive.*;
import org.dbsp.util.ICastable;
import org.dbsp.util.IWritesLogs;
import org.dbsp.util.Logger;
import org.dbsp.sqlCompiler.compiler.errors.UnimplementedException;
import org.dbsp.util.Utilities;

import javax.annotation.Nullable;
import java.util.*;

/**
 * Generate code for the JIT compiler.
 * Handles InnerNodes - i.e., expressions, closures, statements.
 */
public class ToJitInnerVisitor extends InnerVisitor implements IWritesLogs {
    /**
     * Represents the target of an assignment.
     * Could represent either a variable or a set of "out" or "inout" parameters.
     */
    abstract static class AssignmentTarget implements ICastable { }

    /**
     * Can be either a scalar or a tuple variable.
     */
    static class VariableAssignmentTarget extends AssignmentTarget {
        public final String varName;
        public final JITType type;
        public final JITInstructionPair storage;

        VariableAssignmentTarget(String varName, JITType type, JITInstructionPair storage) {
            this.varName = varName;
            this.type = type;
            this.storage = storage;
        }

        @Override
        public String toString() {
            return this.varName + "(" + storage + ")";
        }
    }

    /**
     * Represents a sequence of out or inout parameters.
     */
    static class OutParamsAssignmentTarget extends AssignmentTarget {
        private final List<AssignmentTarget> fields = new ArrayList<>();

        @SuppressWarnings("BooleanMethodIsAlwaysInverted")
        public boolean isEmpty() {
            return this.fields.isEmpty();
        }

        public void addField(AssignmentTarget field) {
            this.fields.add(field);
        }

        public AssignmentTarget getField(int index) {
            if (this.fields.size() <= index)
                throw new InternalCompilerError("Index out of bounds " + index, CalciteObject.EMPTY);
            return this.fields.get(index);
        }

        @Override
        public String toString() {
            return this.fields.toString();
        }

        public int size() {
            return this.fields.size();
        }
    }

    /**
     * Contexts keep track of variables defined.
     * Variable names can be redefined, as in Rust, and a context
     * will always return the one in the current scope.
     */
    class Context {
        /**
         * Maps variable names to expression ids.
         */
        final Map<String, JITInstructionPair> variables;

        public Context() {
            this.variables = new HashMap<>();
        }

        @Nullable
        JITInstructionPair lookup(String varName) {
            if (this.variables.containsKey(varName))
                return this.variables.get(varName);
            return null;
        }

        /**
         * Add a new variable to the current context.
         * @param varName   Variable name.
         * @param needsNull True if the variable is a scalar nullable variable.
         *                  Then we allocate an extra value to hold its
         *                  nullability.
         */
        JITInstructionPair addVariable(String varName, boolean needsNull) {
            if (this.variables.containsKey(varName)) {
                throw new InternalCompilerError("Duplicate declaration" + varName, CalciteObject.EMPTY);
            }
            JITInstructionRef value = ToJitInnerVisitor.this.nextId();
            JITInstructionRef isNull = JITInstructionRef.INVALID;
            if (needsNull) {
                isNull = ToJitInnerVisitor.this.nextId();
            }
            JITInstructionPair result = new JITInstructionPair(value, isNull);
            this.variables.put(varName, result);
            return result;
        }

        /**
         * Add a variable whose definition is known.
         * @param varName  Name of variable.
         * @param pair     Expression that computed the value of the variable.
         */
        void addVariable(String varName, JITInstructionPair pair) {
            this.variables.put(varName, pair);
        }
    }

    /**
     * Used to allocate ids for expressions and blocks.
     */
    private long nextInstrId = 1;
    /**
     * Write here the blocks generated.
     */
    final List<JITBlock> blocks;
    /**
     * Maps each expression to the values that it produces.
     */
    final Map<DBSPExpression, JITInstructionPair> expressionToValues;
    /**
     * A context for each block expression.
     */
    public final List<Context> declarations;

    @Nullable
    JITBlock currentBlock;
    /**
     * The type catalog shared with the ToJitVisitor.
     */
    public final TypeCatalog typeCatalog;
    /**
     * The LHS of the assignments currently under execution.
     * This is a stack, because we can have nested blocks:
     * let var1 = { let v2 = 1 + 2; v2 + 1 }
     */
    final List<AssignmentTarget> variablesAssigned;
    /**
     * A description how closure parameters are mapped to JIT parameters.
     */
    final JITParameterMapping mapping;
    final ToJitVisitor jitVisitor;

    public ToJitInnerVisitor(IErrorReporter reporter, List<JITBlock> blocks,
                             TypeCatalog typeCatalog, ToJitVisitor parent,
                             JITParameterMapping mapping) {
        super(reporter);
        this.blocks = blocks;
        this.jitVisitor = parent;
        this.typeCatalog = typeCatalog;
        this.expressionToValues = new HashMap<>();
        this.declarations = new ArrayList<>();
        this.currentBlock = null;
        this.mapping = mapping;
        this.variablesAssigned = new ArrayList<>();
    }

    void popVariable(AssignmentTarget target) {
        AssignmentTarget last = Utilities.removeLast(this.variablesAssigned);
        if (!last.equals(target))
            throw new InternalCompilerError(
                    "Popping wrong item from assignment stack:" + target + " vs " + last, CalciteObject.EMPTY);
    }

    void pushVariable(AssignmentTarget target) {
        this.variablesAssigned.add(target);
    }

    long nextInstructionId() {
        long result = this.nextInstrId;
        this.nextInstrId++;
        return result;
    }

    JITInstructionRef nextId() {
        long id = this.nextInstructionId();
        return new JITInstructionRef(id);
    }

    JITInstructionPair resolve(String varName) {
        // Look in the contexts in backwards order
        for (int i = 0; i < this.declarations.size(); i++) {
            int index = this.declarations.size() - 1 - i;
            Context current = this.declarations.get(index);
            JITInstructionPair ids = current.lookup(varName);
            if (ids != null)
                return ids;
        }
        throw new InternalCompilerError("Could not resolve " + varName, CalciteObject.EMPTY);
    }

    void map(DBSPExpression expression, JITInstructionPair pair) {
        Logger.INSTANCE.belowLevel(this, 2)
                .append("Compiled ")
                .append(expression.toString())
                .append(" to ")
                .append(pair.toString())
                .newline();
        Utilities.putNew(this.expressionToValues, expression, pair);
    }

    JITInstructionPair getExpressionValues(DBSPExpression expression) {
        return Utilities.getExists(this.expressionToValues, expression);
    }

    JITInstructionPair accept(DBSPExpression expression) {
        expression.accept(this);
        return this.getExpressionValues(expression);
    }

    public JITInstructionRef constantBool(boolean value) {
        JITInstructionRef exists = this.getCurrentBlock().getBooleanConstant(value);
        if (exists.isValid())
            return exists;
        return this.accept(new DBSPBoolLiteral(value)).value;
    }

    public JITScalarType convertScalarType(DBSPExpression expression) {
        return this.jitVisitor.scalarType(expression.getType());
    }

    JITInstruction add(JITInstruction instruction) {
        this.getCurrentBlock().add(instruction);
        return instruction;
    }

    void newContext() {
        this.declarations.add(new Context());
    }

    Context getCurrentContext() {
        return this.declarations.get(this.declarations.size() - 1);
    }

    void popContext() {
        Utilities.removeLast(this.declarations);
    }

    JITInstructionPair declare(String var, boolean needsNull) {
        return this.getCurrentContext().addVariable(var, needsNull);
    }

    /**
     * True if this type needs to store an "is_null" value in a separate place.
     * Tuples don't.  Only scalar nullable types may.
     */
    static boolean needsNull(DBSPType type) {
        if (type.is(DBSPTypeTuple.class))
            return false;
        return type.mayBeNull;
    }

    static boolean needsNull(DBSPExpression expression) {
        return needsNull(expression.getType());
    }

    public JITType convertType(DBSPType type) {
        if (this.jitVisitor.isScalarType(type)) {
            return this.jitVisitor.scalarType(type);
        } else {
            return this.typeCatalog.convertTupleType(type, this.jitVisitor);
        }
    }

    public JITBlock getCurrentBlock() {
        return Objects.requireNonNull(this.currentBlock);
    }

    /**
     * Insert a function call operation.
     * The function call is applied only when all arguments are non-null,
     * otherwise the result is immediately null.
     * @param name           Name of function to call.
     * @param resultType     Expected result type.
     * @param expression     Operations which is being translated.
     *                       Usually an ApplyExpression, but not always.
     * @param arguments      Arguments to supply to the function call.
     */
    JITInstructionPair createFunctionCall(String name,
                            @Nullable DBSPType resultType,
                            DBSPExpression expression,
                            DBSPExpression... arguments) {
        Objects.requireNonNull(resultType);
        List<JITInstructionRef> nullableArgs = new ArrayList<>();
        List<JITType> argumentTypes = new ArrayList<>();
        List<JITInstructionRef> args = new ArrayList<>();
        for (DBSPExpression arg: arguments) {
            JITInstructionPair argValues = this.accept(arg);
            DBSPType argType = arg.getType();
            args.add(argValues.value);
            argumentTypes.add(this.convertType(argType));
            if (argValues.hasNull())
                nullableArgs.add(argValues.isNull);
        }

        // Is any arg nullable?
        JITInstructionRef isNull = JITInstructionRef.INVALID;
        for (JITInstructionRef arg: nullableArgs) {
            if (!isNull.isValid())
                isNull = arg;
            else  {
                isNull = this.insertBinary(JITBinaryInstruction.Operation.OR,
                        isNull, arg, JITBoolType.INSTANCE, "");
            }
        }

        // If any operand is null we create a branch and only call the function
        // conditionally.
        // if (anyNull) { result = default; } else { result = funcCall(args); }
        JITBlock onNullBlock = null;
        JITBlock onNonNullBlock = null;
        JITBlock nextBlock = this.getCurrentBlock();
        if (isNull.isValid()) {
            onNullBlock = this.newBlock();
            onNonNullBlock = this.newBlock();
            nextBlock = this.newBlock();
            JITBranchTerminator branch = new JITBranchTerminator(
                    isNull, onNullBlock.createDestination(),
                    onNonNullBlock.createDestination());
            this.getCurrentBlock().terminate(branch);
            this.setCurrentBlock(onNonNullBlock);
        }

        JITScalarType jitResultType = this.convertType(resultType).to(JITScalarType.class);
        long id = this.nextInstructionId();
        JITInstruction call = this.add(new JITFunctionCall(id, name, args, argumentTypes, jitResultType));
        JITInstructionPair result;

        if (isNull.isValid()) {
            Objects.requireNonNull(nextBlock);
            Objects.requireNonNull(onNonNullBlock);
            Objects.requireNonNull(onNullBlock);
            JITInstructionRef param = this.addParameter(nextBlock, jitResultType);

            JITBlockDestination next = nextBlock.createDestination();
            next.addArgument(call.getInstructionReference());
            JITJumpTerminator terminator = new JITJumpTerminator(next);
            onNonNullBlock.terminate(terminator);

            next = nextBlock.createDestination();
            this.setCurrentBlock(onNullBlock);
            DBSPLiteral defaultValue = resultType
                    .setMayBeNull(false)
                    .to(DBSPTypeBaseType.class)
                    .defaultValue();
            JITInstructionPair defJitValue = this.accept(defaultValue);
            next.addArgument(defJitValue.value);
            terminator = new JITJumpTerminator(next);
            onNullBlock.terminate(terminator);

            this.setCurrentBlock(nextBlock);
            result = new JITInstructionPair(param, isNull);
        } else {
            result = new JITInstructionPair(call);
        }
        return result;
    }

    /**
     * Insert a function call operation.
     * The function call is applied only when all arguments are non-null,
     * otherwise the result is immediately null.
     * @param name           Name of function to call.
     * @param expression     Operations which is being translated.
     *                       Usually an ApplyExpression, but not always.
     * @param arguments      Arguments to supply to the function call.
     */
    JITInstructionPair createFunctionCall(String name,
                                          DBSPExpression expression,
                                          DBSPExpression... arguments) {
        return this.createFunctionCall(name, expression.getType(), expression, arguments);
    }

    /////////////////////////// Code generation

    @Override
    public VisitDecision preorder(DBSPExpression expression) {
        throw new UnimplementedException(expression);
    }

    @Override
    public VisitDecision preorder(DBSPSomeExpression expression) {
        JITInstructionPair source = this.accept(expression.expression);
        JITInstructionRef False = this.constantBool(false);
        JITInstructionPair result = new JITInstructionPair(source.value, False);
        this.map(expression, result);
        return VisitDecision.STOP;
    }

    /**
     * Function Type class.  Describes some type information about a JIT function.
     * (Chose a short name to make the hashmap below more compact.)
     */
    static class FT {
        /**
         * The name of the JIT function.
         */
        public final String name;
        /**
         * Type of result produced by JIT function.
         */
        public final DBSPTypeBaseType resultType;

        FT(String name, DBSPTypeBaseType resultType) {
            this.name = name;
            this.resultType = resultType;
        }
    }
    
    static final Map<String, FT> functionTranslation = new HashMap<String, FT>() {{
        put("extract_second_Timestamp", new FT("dbsp.timestamp.second", DBSPTypeInteger.SIGNED_64));
        put("extract_minute_Timestamp", new FT("dbsp.timestamp.minute", DBSPTypeInteger.SIGNED_64));
        put("extract_hour_Timestamp", new FT("dbsp.timestamp.hour", DBSPTypeInteger.SIGNED_64));
        put("extract_day_Timestamp", new FT("dbsp.timestamp.day", DBSPTypeInteger.SIGNED_64));
        put("extract_dow_Timestamp", new FT("dbsp.timestamp.day_of_week", DBSPTypeInteger.SIGNED_64));
        put("extract_doy_Timestamp", new FT("dbsp.timestamp.day_of_year", DBSPTypeInteger.SIGNED_64));
        put("extract_isodow_Timestamp", new FT("dbsp.timestamp.iso_day_of_week", DBSPTypeInteger.SIGNED_64));
        put("extract_week_Timestamp", new FT("dbsp.timestamp.week", DBSPTypeInteger.SIGNED_64));
        put("extract_month_Timestamp", new FT("dbsp.timestamp.month", DBSPTypeInteger.SIGNED_64));
        put("extract_year_Timestamp", new FT("dbsp.timestamp.year", DBSPTypeInteger.SIGNED_64));
        put("extract_isoyear_Timestamp", new FT("dbsp.timestamp.iso_year", DBSPTypeInteger.SIGNED_64));
        put("extract_quarter_Timestamp", new FT("dbsp.timestamp.quarter", DBSPTypeInteger.SIGNED_64));
        put("extract_decade_Timestamp", new FT("dbsp.timestamp.decade", DBSPTypeInteger.SIGNED_64));
        put("extract_century_Timestamp", new FT("dbsp.timestamp.century", DBSPTypeInteger.SIGNED_64));
        put("extract_millennium_Timestamp", new FT("dbsp.timestamp.millennium", DBSPTypeInteger.SIGNED_64));
        put("extract_epoch_Timestamp", new FT("dbsp.timestamp.epoch", DBSPTypeInteger.SIGNED_32));
        put("extract_second_Date", new FT("dbsp.date.second", DBSPTypeInteger.SIGNED_32));
        put("extract_millisecond_Date", new FT("dbsp.date.millisecond", DBSPTypeInteger.SIGNED_32));
        put("extract_microsecond_Date", new FT("dbsp.date.microsecond", DBSPTypeInteger.SIGNED_32));
        put("extract_minute_Date", new FT("dbsp.date.minute", DBSPTypeInteger.SIGNED_32));
        put("extract_hour_Date", new FT("dbsp.date.hour", DBSPTypeInteger.SIGNED_32));
        put("extract_day_Date", new FT("dbsp.date.day", DBSPTypeInteger.SIGNED_32));
        put("extract_dow_Date", new FT("dbsp.date.day_of_week", DBSPTypeInteger.SIGNED_32));
        put("extract_doy_Date", new FT("dbsp.date.day_of_year", DBSPTypeInteger.SIGNED_32));
        put("extract_isodow_Date", new FT("dbsp.date.iso_day_of_week", DBSPTypeInteger.SIGNED_32));
        put("extract_week_Date", new FT("dbsp.date.week", DBSPTypeInteger.SIGNED_32));
        put("extract_month_Date", new FT("dbsp.date.month", DBSPTypeInteger.SIGNED_32));
        put("extract_year_Date", new FT("dbsp.date.year", DBSPTypeInteger.SIGNED_32));
        put("extract_isoyear_Date", new FT("dbsp.date.iso_year", DBSPTypeInteger.SIGNED_32));
        put("extract_quarter_Date", new FT("dbsp.date.quarter", DBSPTypeInteger.SIGNED_32));
        put("extract_decade_Date", new FT("dbsp.date.decade", DBSPTypeInteger.SIGNED_32));
        put("extract_century_Date", new FT("dbsp.date.century", DBSPTypeInteger.SIGNED_32));
        put("extract_millennium_Date", new FT("dbsp.date.millennium", DBSPTypeInteger.SIGNED_32));
        put("extract_epoch_Date", new FT("dbsp.date.epoch", DBSPTypeInteger.SIGNED_32));
    }};

    @Override
    public VisitDecision preorder(DBSPApplyExpression expression) {
        JITScalarType resultType = this.convertScalarType(expression);
        DBSPPathExpression path = expression.function.as(DBSPPathExpression.class);
        if (path != null) {
            String function = path.path.toString();
            if (function.endsWith("N"))
                function = function.substring(0, function.length() - 1);
            FT jitFunction = functionTranslation.get(function);
            if (jitFunction != null) {
                JITInstructionPair call = this.createFunctionCall(
                        jitFunction.name, jitFunction.resultType, expression, expression.arguments);
                JITScalarType type = this.convertType(jitFunction.resultType).to(JITScalarType.class);
                JITInstructionRef cast = this.insertCast(call.value, type, resultType, "");
                JITInstructionPair result = new JITInstructionPair(cast, call.isNull);
                this.map(expression, result);
                return VisitDecision.STOP;
            }
        }
        throw new UnimplementedException(expression);
    }

    @Override
    public VisitDecision preorder(DBSPLiteral expression) {
        JITScalarType type = convertScalarType(expression);
        JITLiteral literal = new JITLiteral(expression, type);
        boolean mayBeNull = expression.getType().mayBeNull;
        JITConstantInstruction value = new JITConstantInstruction(
                this.nextInstructionId(), type, literal, true);
        this.add(value);

        JITInstructionRef isNull = JITInstructionRef.INVALID;
        if (mayBeNull) {
            isNull = this.constantBool(expression.isNull);
        }
        JITInstructionPair pair = new JITInstructionPair(value.getInstructionReference(), isNull);
        this.map(expression, pair);
        return VisitDecision.STOP;
    }

    public VisitDecision preorder(DBSPBorrowExpression expression) {
        JITInstructionPair sourceId = this.accept(expression.expression);
        Utilities.putNew(this.expressionToValues, expression, sourceId);
        return VisitDecision.STOP;
    }

    @Override
    public VisitDecision preorder(DBSPCastExpression expression) {
        JITScalarType sourceType = convertScalarType(expression.source);
        JITScalarType destinationType = convertScalarType(expression);
        if (destinationType.is(JITStringType.class) &&
                (sourceType.is(JITDateType.class) || sourceType.is(JITTimestampType.class))) {
            // These are implemented using function calls.
            DBSPExpression empty = new DBSPStringLiteral("").applyClone();
            // Write appends the argument to the supplied string
            JITInstructionPair result = this.createFunctionCall(
                    "dbsp.str.write", expression, empty, expression.source);
            this.map(expression, result);
            return VisitDecision.STOP;
        }
        if (sourceType.is(JITStringType.class) && !destinationType.is(JITStringType.class)) {
            JITInstructionPair result = this.createFunctionCall(
                    "dbsp.str.parse", expression, expression.source);
            this.map(expression, result);
            return VisitDecision.STOP;
        }

        JITInstructionPair sourceId = this.accept(expression.source);
        JITInstructionRef cast = this.insertCast(sourceId.value, sourceType, destinationType, expression.toString());
        JITInstructionRef isNull = JITInstructionRef.INVALID;
        if (needsNull(expression)) {
            if (needsNull(expression.source)) {
                isNull = sourceId.isNull;
            } else {
                isNull = this.constantBool(false);
            }
        } else {
            if (needsNull(expression.source)) {
                // TODO: if source is nullable and is null must panic at runtime
                // this.createFunctionCall("dbsp.error.abort", expression);
            }
            // else nothing to do for null field
        }
        this.map(expression, new JITInstructionPair(cast, isNull));
        return VisitDecision.STOP;
    }

    static final Map<DBSPOpcode, JITBinaryInstruction.Operation> opNames = new HashMap<>();

    static {
        opNames.put(DBSPOpcode.ADD, JITBinaryInstruction.Operation.ADD);
        opNames.put(DBSPOpcode.AGG_ADD, JITBinaryInstruction.Operation.ADD);
        opNames.put(DBSPOpcode.AGG_MAX, JITBinaryInstruction.Operation.MAX);
        opNames.put(DBSPOpcode.AGG_MIN, JITBinaryInstruction.Operation.MIN);
        opNames.put(DBSPOpcode.SUB, JITBinaryInstruction.Operation.SUB);
        opNames.put(DBSPOpcode.MUL, JITBinaryInstruction.Operation.MUL);
        opNames.put(DBSPOpcode.DIV, JITBinaryInstruction.Operation.DIV);
        opNames.put(DBSPOpcode.EQ,JITBinaryInstruction.Operation.EQ);
        opNames.put(DBSPOpcode.NEQ, JITBinaryInstruction.Operation.NEQ);
        opNames.put(DBSPOpcode.LT, JITBinaryInstruction.Operation.LT);
        opNames.put(DBSPOpcode.GT, JITBinaryInstruction.Operation.GT);
        opNames.put(DBSPOpcode.LTE, JITBinaryInstruction.Operation.LTE);
        opNames.put(DBSPOpcode.GTE, JITBinaryInstruction.Operation.GTE);
        opNames.put(DBSPOpcode.BW_AND, JITBinaryInstruction.Operation.AND);
        opNames.put(DBSPOpcode.AND, JITBinaryInstruction.Operation.AND);
        opNames.put(DBSPOpcode.BW_OR, JITBinaryInstruction.Operation.OR);
        opNames.put(DBSPOpcode.OR, JITBinaryInstruction.Operation.OR);
        opNames.put(DBSPOpcode.XOR, JITBinaryInstruction.Operation.XOR);
        opNames.put(DBSPOpcode.MAX, JITBinaryInstruction.Operation.MAX);
        opNames.put(DBSPOpcode.MIN, JITBinaryInstruction.Operation.MIN);
    }

    /**
     * Insert an instruction which computes left.isNull OR right.isNull
     * but which works when either is not nullable.  Return a reference to this instruction.
     */
    JITInstructionRef eitherNull(JITInstructionPair left, JITInstructionPair right) {
        if (left.hasNull()) {
            if (right.hasNull()) {
                return this.insertBinary(JITBinaryInstruction.Operation.OR,
                        left.isNull, right.isNull, JITBoolType.INSTANCE, "");
            } else {
                return left.isNull;
            }
        } else {
            if (right.hasNull()) {
                return right.isNull;
            } else {
                return this.constantBool(false);
            }
        }
    }

    JITInstructionRef insertCast(JITInstructionRef source,
                                 JITScalarType sourceType, JITScalarType destType, String comment) {
        if (sourceType.equals(destType))
            // This can happen for casts that only change nullability
            return source;
        JITInstruction cast = this.add(new JITCastInstruction(
                this.nextInstructionId(), source, sourceType, destType, comment));
        return cast.getInstructionReference();
    }

    /**
     * Insert a mux operation in the current block.
     * @param condition  Reference to instruction computing mux condition.
     * @param left       Reference to the left operand.
     * @param right      Reference to the right operand.
     * @param valueType  Type of the value produced by the mux.
     * @return           A reference to the inserted mux.  If the mux condition
     *                   can be determined to be constant only one of the
     *                   operands may be returned.
     */
    @SuppressWarnings("SameParameterValue")
    JITInstructionRef insertMux(
            JITInstructionRef condition,
            JITInstructionRef left, JITInstructionRef right,
            JITType valueType, String comment) {
        condition.mustBeValid();
        left.mustBeValid();
        right.mustBeValid();
        JITInstructionRef False = this.getCurrentBlock().getBooleanConstant(false);
        JITInstructionRef True = this.getCurrentBlock().getBooleanConstant(true);

        if (condition.equals(False))
            return right;
        if (condition.equals(True))
            return left;
        JITInstruction mux = this.add(new JITMuxInstruction(this.nextInstructionId(),
                condition, left, right, valueType, comment));
        return mux.getInstructionReference();
    }

    JITInstructionRef insertUnary(JITUnaryInstruction.Operation opcode,
                                  JITInstructionRef operand, JITType type) {
        operand.mustBeValid();
        JITInstruction result = this.add(new JITUnaryInstruction(this.nextInstructionId(), opcode, operand, type));
        return result.getInstructionReference();
    }

    JITInstructionRef insertBinary(JITBinaryInstruction.Operation opcode,
                                JITInstructionRef left, JITInstructionRef right,
                                JITType type, String comment) {
        left.mustBeValid();
        right.mustBeValid();
        JITInstruction result = this.add(new JITBinaryInstruction(
                this.nextInstructionId(), opcode, left, right, type, comment));
        return result.getInstructionReference();
    }

    JITInstructionRef addParameter(JITBlock block, JITType type) {
        return block.addParameter(new JITInstructionRef(this.nextInstructionId()), type);
    }

    @Override
    public VisitDecision preorder(DBSPBinaryExpression expression) {
        if (expression.operation.equals(DBSPOpcode.CONCAT)) {
            JITInstructionPair result = this.createFunctionCall(
                    "dbsp.str.concat", expression,
                    expression.left, expression.right);
            this.map(expression, result);
            return VisitDecision.STOP;
        }
        DBSPType leftType = expression.left.getType();
        if (expression.operation.equals(DBSPOpcode.DIV) &&
                (leftType.is(DBSPTypeInteger.class) || leftType.is(DBSPTypeDecimal.class))) {
            // left / right
            JITInstructionPair left = this.accept(expression.left);
            JITInstructionPair right = this.accept(expression.right);

            // division by 0 returns null.
            IsNumericType numeric = leftType.to(IsNumericType.class);
            JITScalarType type = convertScalarType(expression.left);
            // TODO: replace with uninit
            DBSPLiteral numericZero = numeric.getZero();
            JITLiteral jitZero = new JITLiteral(numericZero, type);
            JITInstruction zero = this.add(
                    new JITConstantInstruction(this.nextInstructionId(), type, jitZero, true));
            // (right == 0)
            JITInstructionRef compare = this.insertBinary(JITBinaryInstruction.Operation.EQ,
                    zero.getInstructionReference(), right.value, type, "");
            JITBlock isZero = this.newBlock();
            JITBlock isNotZero = this.newBlock();
            JITBlock next = this.newBlock();
            JITBlockDestination isZeroDestination = isZero.createDestination();
            JITBlockDestination isNotZeroDestination = isNotZero.createDestination();
            JITBranchTerminator branch = new JITBranchTerminator(
                    compare, isZeroDestination, isNotZeroDestination);
            this.getCurrentBlock().terminate(branch);

            // if (right == 0)
            this.setCurrentBlock(isZero);
            JITInstructionRef True = this.constantBool(true);
            // isNull = true
            JITBlockDestination zeroToNext = next.createDestination();
            zeroToNext.addArgument(True);
            // result can be 0
            zeroToNext.addArgument(zero.getInstructionReference());
            JITJumpTerminator jump = new JITJumpTerminator(zeroToNext);
            isZero.terminate(jump);

            // else
            this.setCurrentBlock(isNotZero);
            // isNull = left.isNull || right.isNull
            JITInstructionRef isNull = this.eitherNull(left, right);
            // result = left / right (even if either is null this is hopefully fine).
            JITInstructionRef div = this.insertBinary(JITBinaryInstruction.Operation.DIV,
                            left.value, right.value, type, expression.toString());
            JITBlockDestination isNotZeroToNext = next.createDestination();
            isNotZeroToNext.addArgument(isNull);
            isNotZeroToNext.addArgument(div);
            jump = new JITJumpTerminator(isNotZeroToNext);
            isNotZero.terminate(jump);

            // join point
            this.setCurrentBlock(next);
            JITInstructionRef resultIsNull = this.addParameter(next, JITBoolType.INSTANCE);
            JITInstructionRef value = this.addParameter(next, type);
            JITInstructionPair result = new JITInstructionPair(value, resultIsNull);
            this.map(expression, result);
            return VisitDecision.STOP;
        }

        JITInstructionPair leftId = this.accept(expression.left);
        JITInstructionPair rightId = this.accept(expression.right);
        boolean special = expression.operation.equals(DBSPOpcode.AND) ||
                (expression.operation.equals(DBSPOpcode.OR));
        if (needsNull(expression) && special) {
            JITInstructionRef False = this.constantBool(false);
            JITInstructionRef True = this.constantBool(true);

            JITInstructionRef leftNullId;
            if (leftId.hasNull())
                leftNullId = leftId.isNull;
            else
                // Not nullable: use false.
                leftNullId = False;
            JITInstructionRef rightNullId;
            if (rightId.hasNull())
                rightNullId = rightId.isNull;
            else
                rightNullId = False;

            if (expression.operation.equals(DBSPOpcode.AND)) {
                // Nullable bit computation
                // (a && b).is_null = a.is_null ? (b.is_null ? true    : b.value)
                //                              : (b.is_null ? a.value : false)

                // cond1 = (b.is_null ? true : b.value)
                this.insertMux(rightNullId, True, rightId.value, JITBoolType.INSTANCE, "");
                JITInstructionRef cond1 = this.insertMux(rightNullId, True, rightId.value, JITBoolType.INSTANCE, "");
                // cond2 = (b.is_null ? !a.value   : false)
                JITInstructionRef cond2 = this.insertMux(rightNullId, leftId.value, False, JITBoolType.INSTANCE, "");

                // (a && b).value = a.is_null ? b.value
                //                            : (b.is_null ? a.value : a.value && b.value)
                // (The value for a.is_null & b.is_null does not matter, so we can choose it to be b.value)
                // a.value && b.value
                JITInstructionRef and = this.insertBinary(
                        JITBinaryInstruction.Operation.AND,
                        leftId.value, rightId.value, convertScalarType(expression.left), "");
                // (b.is_null ? a.value : a.value && b.value)
                JITInstructionRef secondBranch = this.insertMux(rightNullId, leftId.value, and, JITBoolType.INSTANCE, "");
                // Final Mux
                JITInstructionRef value = this.insertMux(
                        leftNullId, rightId.value, secondBranch, JITBoolType.INSTANCE, expression.toString());
                JITInstructionRef isNull = this.insertMux(leftNullId, cond1, cond2, JITBoolType.INSTANCE,
                        expression.is_null().toString());
                this.map(expression, new JITInstructionPair(value, isNull));
            } else { // Boolean ||
                // Nullable bit computation
                // (a || b).is_null = a.is_null ? (b.is_null ? true : !b.value)
                //                              : (b.is_null ? !a.value : false)
                // true
                // cond1 = (b.is_null ? true : !b.value)
                // !b.value
                JITInstructionRef notB = this.insertUnary(
                        JITUnaryInstruction.Operation.NOT, rightId.value, JITBoolType.INSTANCE);
                JITInstructionRef cond1 = this.insertMux(
                        rightNullId, True, notB, JITBoolType.INSTANCE, "");
                // cond2 = (b.is_null ? !a.value : false)
                // !a
                JITInstructionRef notA = this.insertUnary(
                        JITUnaryInstruction.Operation.NOT, leftId.value, JITBoolType.INSTANCE);
                JITInstructionRef cond2 = this.insertMux(rightNullId, notA, False, JITBoolType.INSTANCE, "");

                // (a || b).value = a.is_null ? b.value
                //                            : a.value || b.value
                // a.value || b.value
                JITInstructionRef or = this.insertBinary(
                                JITBinaryInstruction.Operation.OR,
                                leftId.value, rightId.value, convertScalarType(expression.left), "");
                // Result
                JITInstructionRef value = this.insertMux(leftNullId, cond1, cond2, JITBoolType.INSTANCE, expression.toString());
                JITInstructionRef isNull = this.insertMux(leftNullId, rightId.value, or, JITBoolType.INSTANCE,
                        expression.is_null().toString());
                this.map(expression, new JITInstructionPair(value, isNull));
            }
            return VisitDecision.STOP;
        } else if (expression.operation.isAggregate) {
            JITBinaryInstruction.Operation op = Utilities.getExists(opNames, expression.operation);
            // If either operand is null, the result is the other operand.
            if (leftId.hasNull()) {
                JITBlock ifNull = this.newBlock();
                JITBlock ifNotNull = this.newBlock();
                JITBlock next = this.newBlock();
                JITBranchTerminator branch = new JITBranchTerminator(
                        leftId.isNull, ifNull.createDestination(), ifNotNull.createDestination());
                this.getCurrentBlock().terminate(branch);

                this.setCurrentBlock(ifNull);
                JITBlockDestination nextDestination = next.createDestination();
                nextDestination.addArgument(rightId.value);
                nextDestination.addArgument(rightId.hasNull() ? rightId.isNull : this.constantBool(false));
                JITJumpTerminator jump = new JITJumpTerminator(nextDestination);
                this.getCurrentBlock().terminate(jump);

                this.setCurrentBlock(ifNotNull);
                if (rightId.hasNull()) {
                    JITBlock ifRightNull = this.newBlock();
                    JITBlock ifRightNotNull = this.newBlock();
                    branch = new JITBranchTerminator(
                            rightId.isNull, ifRightNull.createDestination(), ifRightNotNull.createDestination());
                    this.getCurrentBlock().terminate(branch);

                    this.setCurrentBlock(ifRightNull);
                    nextDestination = next.createDestination();
                    nextDestination.addArgument(leftId.value);
                    nextDestination.addArgument(leftId.hasNull() ? leftId.isNull : this.constantBool(false));
                    jump = new JITJumpTerminator(nextDestination);
                    this.getCurrentBlock().terminate(jump);

                    this.setCurrentBlock(ifRightNotNull);
                    JITInstructionRef value = this.insertBinary(op, leftId.value, rightId.value,
                            convertScalarType(expression.left), expression.toString());
                    nextDestination = next.createDestination();
                    nextDestination.addArgument(value);
                    nextDestination.addArgument(this.constantBool(false));
                } else {
                    JITInstructionRef value = this.insertBinary(op, leftId.value, rightId.value,
                            convertScalarType(expression.left), expression.toString());
                    nextDestination = next.createDestination();
                    nextDestination.addArgument(value);
                    nextDestination.addArgument(this.constantBool(false));
                }
                jump = new JITJumpTerminator(nextDestination);
                this.getCurrentBlock().terminate(jump);

                this.setCurrentBlock(next);
                JITInstructionRef resultValue = this.addParameter(next, convertScalarType(expression));
                JITInstructionRef resultIsNull = this.addParameter(next, JITBoolType.INSTANCE);
                JITInstructionPair result = new JITInstructionPair(resultValue, resultIsNull);
                this.map(expression, result);
            } else {
                if (rightId.hasNull()) {
                    JITBlock ifNull = this.newBlock();
                    JITBlock ifNotNull = this.newBlock();
                    JITBlock next = this.newBlock();
                    JITBranchTerminator branch = new JITBranchTerminator(
                            rightId.isNull, ifNull.createDestination(), ifNotNull.createDestination());
                    this.getCurrentBlock().terminate(branch);

                    this.setCurrentBlock(ifNull);
                    JITBlockDestination nextDestination = next.createDestination();
                    nextDestination.addArgument(leftId.value);
                    nextDestination.addArgument(leftId.hasNull() ? leftId.isNull : this.constantBool(false));
                    JITJumpTerminator jump = new JITJumpTerminator(nextDestination);
                    this.getCurrentBlock().terminate(jump);

                    this.setCurrentBlock(ifNotNull);
                    JITInstructionRef value = this.insertBinary(op, leftId.value, rightId.value,
                            convertScalarType(expression.left), expression.toString());
                    nextDestination = next.createDestination();
                    nextDestination.addArgument(value);
                    nextDestination.addArgument(this.constantBool(false));
                    jump = new JITJumpTerminator(nextDestination);
                    this.getCurrentBlock().terminate(jump);

                    this.setCurrentBlock(next);
                    JITInstructionRef resultValue = this.addParameter(next, convertScalarType(expression));
                    JITInstructionRef resultIsNull = this.addParameter(next, JITBoolType.INSTANCE);
                    JITInstructionPair result = new JITInstructionPair(resultValue, resultIsNull);
                    this.map(expression, result);
                } else {
                    JITInstructionRef value = this.insertBinary(op, leftId.value, rightId.value,
                            convertScalarType(expression.left), expression.toString());
                    this.map(expression, new JITInstructionPair(value));
                }
            }
            return VisitDecision.STOP;
        } else if (expression.operation.equals(DBSPOpcode.MUL_WEIGHT)) {
            throw new InternalCompilerError("Should have been removed", expression);
        }

        JITInstructionRef value = this.insertBinary(
                Utilities.getExists(opNames, expression.operation), leftId.value, rightId.value,
                convertScalarType(expression.left), expression.toString());
        JITInstructionRef isNull = JITInstructionRef.INVALID;
        if (needsNull(expression))
            // The result is null if either operand is null.
            isNull = this.eitherNull(leftId, rightId);
        this.map(expression, new JITInstructionPair(value, isNull));
        return VisitDecision.STOP;
    }

    @Override
    public VisitDecision preorder(DBSPUnaryExpression expression) {
        JITInstructionPair source = this.accept(expression.source);
        boolean isWrapBool = expression.operation.equals(DBSPOpcode.WRAP_BOOL);
        JITInstructionRef False = JITInstructionRef.INVALID;
        if (isWrapBool || expression.operation.equals(DBSPOpcode.IS_FALSE)
                || expression.operation.equals(DBSPOpcode.IS_TRUE))
            False = this.constantBool(false);
        JITUnaryInstruction.Operation kind;
        switch (expression.operation) {
            case NEG:
                kind = JITUnaryInstruction.Operation.NEG;
                break;
            case NOT:
                kind = JITUnaryInstruction.Operation.NOT;
                break;
            case WRAP_BOOL: {
                JITInstructionRef value = this.insertMux(
                        source.isNull, False, source.value, JITBoolType.INSTANCE, expression.toString());
                this.map(expression, new JITInstructionPair(value));
                return VisitDecision.STOP;
            }
            case IS_FALSE: {
                if (source.hasNull()) {
                    // result = left.is_null ? false : !left.value
                    // ! left.value
                    JITInstructionRef ni = this.insertUnary(
                        JITUnaryInstruction.Operation.NOT, source.value, convertScalarType(expression.source));
                    // result
                    JITInstructionRef value = this.insertMux(
                            source.isNull, False, ni, JITBoolType.INSTANCE, expression.toString());
                    this.map(expression, new JITInstructionPair(value));
                    return VisitDecision.STOP;
                } else {
                    kind = JITUnaryInstruction.Operation.NOT;
                }
                break;
            }
            case IS_TRUE: {
                if (source.hasNull()) {
                    // result = left.is_null ? false : left.value
                    // result
                    JITInstructionRef value = this.insertMux(
                        source.isNull, False, source.value, JITBoolType.INSTANCE, expression.toString());
                    this.map(expression, new JITInstructionPair(value));
                } else {
                    this.map(expression, new JITInstructionPair(source.value));
                }
                return VisitDecision.STOP;
            }
            case IS_NOT_TRUE: {
                if (source.hasNull()) {
                    // result = left.is_null ? true : !left.value
                    // ! left.value
                    JITInstructionRef ni = this.insertUnary(
                        JITUnaryInstruction.Operation.NOT, source.value, convertScalarType(expression.source));
                    JITInstructionRef True = this.constantBool(true);
                    // result
                    JITInstructionRef value = this.insertMux(
                            source.isNull, True, ni, JITBoolType.INSTANCE, expression.toString());
                    this.map(expression, new JITInstructionPair(value));
                    return VisitDecision.STOP;
                } else {
                    kind = JITUnaryInstruction.Operation.NOT;
                }
                break;
            }
            case IS_NOT_FALSE: {
                if (source.hasNull()) {
                    // result = left.is_null ? true : left.value
                    JITInstructionRef True = this.constantBool(true);
                    // result
                    JITInstructionRef value = this.insertMux(
                        source.isNull, True, source.value, JITBoolType.INSTANCE, expression.toString());
                    this.map(expression, new JITInstructionPair(value));
                } else {
                    this.map(expression, new JITInstructionPair(source.value));
                }
                return VisitDecision.STOP;
            }
            case INDICATOR: {
                if (!source.hasNull())
                    throw new InternalCompilerError("indicator called on non-nullable expression", expression);
                JITInstructionRef value = this.insertCast(
                    source.isNull, JITBoolType.INSTANCE, JITI64Type.INSTANCE, "");
                this.map(expression, new JITInstructionPair(value));
                return VisitDecision.STOP;
            }
            default:
                throw new UnimplementedException(expression);
        }
        JITInstructionRef value = this.insertUnary(kind, source.value, convertScalarType(expression.source));
        JITInstructionRef isNull = JITInstructionRef.INVALID;
        if (source.hasNull())
            isNull = source.isNull;
        this.map(expression, new JITInstructionPair(value, isNull));
        return VisitDecision.STOP;
    }

    @Override
    public VisitDecision preorder(DBSPVariablePath expression) {
        JITInstructionPair pair = this.resolve(expression.variable);
        // may already be there, but this may be a new variable with the same name,
        // and then we overwrite with the new definition.
        this.expressionToValues.put(expression, pair);
        return VisitDecision.STOP;
    }

    @Override
    public VisitDecision preorder(DBSPClosureExpression closure) {
        OutParamsAssignmentTarget target = new OutParamsAssignmentTarget();
        for (JITParameter param: this.mapping.parameters) {
            JITInstructionPair dest = this.declare(param.originalName, param.mayBeNull);
            VariableAssignmentTarget var = new VariableAssignmentTarget(param.originalName, param.type, dest);
            if (param.direction != JITParameter.Direction.IN) {
                target.addField(var);
            }
        }
        if (!target.isEmpty())
            this.pushVariable(target);

        closure.body.accept(this);

        if (!target.isEmpty())
            this.popVariable(target);
        return VisitDecision.STOP;
    }

    @Override
    public VisitDecision preorder(DBSPLetStatement statement) {
        boolean isTuple = statement.type.is(DBSPTypeTupleBase.class);
        JITType type = this.convertType(statement.type);
        VariableAssignmentTarget var = null;
        if (isTuple) {
            // Do not declare scalar variables, just substitute them with the
            // expression that the RHS produces.
            JITInstructionPair declaration = this.declare(statement.variable, needsNull(statement.type));
            var = new VariableAssignmentTarget(statement.variable, type, declaration);
            this.pushVariable(var);
            this.add(new JITUninitInstruction(declaration.value.getId(), type.to(JITRowType.class), statement.variable));
        }
        if (statement.initializer != null) {
            statement.initializer.accept(this);
            if (!isTuple) {
                // Only if the expression produces a scalar value will there be an init.
                // Otherwise, the tuple expression will directly write into the variable
                JITInstructionPair init = this.getExpressionValues(statement.initializer);
                this.getCurrentContext().addVariable(statement.variable, init);
            }
        }

        if (isTuple)
            this.popVariable(Objects.requireNonNull(var));
        return VisitDecision.STOP;
    }

    @Override
    public VisitDecision preorder(DBSPFieldExpression expression) {
        JITInstruction isNull = null;
        JITInstructionPair sourceId = this.accept(expression.expression);
        JITRowType sourceType = this.typeCatalog.convertTupleType(
                expression.expression.getType(), this.jitVisitor);
        JITInstruction load = this.add(new JITLoadInstruction(
                this.nextInstructionId(), sourceId.value, sourceType,
                expression.fieldNo, convertScalarType(expression), expression.toString()));
        if (needsNull(expression)) {
            isNull = this.add(new JITIsNullInstruction(this.nextInstructionId(), sourceId.value,
                sourceType, expression.fieldNo));
        }
        this.map(expression, new JITInstructionPair(load, isNull));
        return VisitDecision.STOP;
    }

    @Override
    public VisitDecision preorder(DBSPIsNullExpression expression) {
        JITInstructionPair sourceId = this.accept(expression.expression);
        this.map(expression, new JITInstructionPair(sourceId.isNull));
        return VisitDecision.STOP;
    }

    @Override
    public VisitDecision preorder(DBSPCloneExpression expression) {
        JITInstructionPair source = this.accept(expression.expression);
        if (expression.getType().hasCopy()) {
            this.map(expression, source);
            return VisitDecision.STOP;
        }

        JITType type = this.convertType(expression.getType());
        if (type.isScalarType()) {
            JITScalarType scalarType = type.to(JITScalarType.class);
            if (!needsNull(expression)) {
                JITInstruction copy = this.add(new JITCopyInstruction(this.nextInstructionId(), source.value, scalarType));
                this.map(expression, new JITInstructionPair(copy));
                return VisitDecision.STOP;
            } else {
                JITBlock isNull = this.newBlock();
                JITBlock isNotNull = this.newBlock();
                JITBlock next = this.newBlock();
                JITBranchTerminator branch = new JITBranchTerminator(
                        source.isNull, isNull.createDestination(), isNotNull.createDestination());
                this.getCurrentBlock().terminate(branch);
                this.setCurrentBlock(isNull);

                JITInstruction uninit = this.add(new JITUninitInstruction(
                        this.nextInstructionId(), scalarType, "if (" + expression + ").is_null"));
                JITBlockDestination nextDestination = next.createDestination();
                nextDestination.addArgument(uninit.getInstructionReference());
                JITJumpTerminator jump = new JITJumpTerminator(nextDestination);
                this.getCurrentBlock().terminate(jump);

                this.setCurrentBlock(isNotNull);
                JITInstruction copy = this.add(new JITCopyInstruction(this.nextInstructionId(), source.value, scalarType));
                nextDestination = next.createDestination();
                nextDestination.addArgument(copy.getInstructionReference());
                jump = new JITJumpTerminator(nextDestination);
                this.getCurrentBlock().terminate(jump);

                this.setCurrentBlock(next);
                JITInstructionRef param = this.addParameter(next, scalarType);
                JITInstructionPair result = new JITInstructionPair(param, source.isNull);
                this.map(expression, result);
            }
        } else {
            throw new UnimplementedException("Clone with non-scalar type ", expression);
        }
        return VisitDecision.STOP;
    }

    @Override
    public VisitDecision preorder(DBSPIfExpression expression) {
        JITInstructionPair cond = this.accept(expression.condition);
        JITBlock ifTrue = this.newBlock();
        JITBlock ifFalse = this.newBlock();
        boolean nullable = needsNull(expression);

        JITBlock next = this.newBlock();
        JITBranchTerminator branch = new JITBranchTerminator(
                cond.value, ifTrue.createDestination(), ifFalse.createDestination());
        this.getCurrentBlock().terminate(branch);

        this.setCurrentBlock(ifTrue);
        JITInstructionPair positive = this.accept(expression.positive);
        JITBlockDestination nextDest = next.createDestination();
        nextDest.addArgument(positive.value);
        if (nullable)
            nextDest.addArgument(positive.isNull);
        JITJumpTerminator jump = new JITJumpTerminator(nextDest);
        this.getCurrentBlock().terminate(jump);

        this.setCurrentBlock(ifFalse);
        JITInstructionPair negative = this.accept(expression.negative);
        nextDest = next.createDestination();
        nextDest.addArgument(negative.value);
        if (nullable)
            nextDest.addArgument(negative.isNull);
        jump = new JITJumpTerminator(nextDest);
        this.getCurrentBlock().terminate(jump);

        this.setCurrentBlock(next);
        JITType type = this.convertType(expression.getType());
        JITInstructionRef paramValue = new JITInstructionRef(this.nextInstructionId());
        JITInstructionRef isNull = JITInstructionRef.INVALID;
        this.addParameter(next, type);
        if (nullable) {
            this.addParameter(next, JITBoolType.INSTANCE);
        }
        this.setCurrentBlock(next);
        this.map(expression, new JITInstructionPair(paramValue, isNull));
        return VisitDecision.STOP;
    }

    @Override
    public VisitDecision preorder(DBSPBaseTupleExpression expression) {
        // Compile this as an assignment to the currently assigned variables.
        // We have several cases:
        // var v = Tuple::new(x, y)  (Tuple-typed)
        // var v = (x, y);           (RawTuple-typed, but non-tuple fields)
        // return Tuple::new(x, y)   (Tuple-typed, last expression in outermost block of a closure).
        // return (x, y);            (RowTuple-typed, last expression in outermost block).
        AssignmentTarget target = Utilities.last(this.variablesAssigned);
        VariableAssignmentTarget varTarget = target.as(VariableAssignmentTarget.class);
        OutParamsAssignmentTarget paramsTarget = target.as(OutParamsAssignmentTarget.class);
        int index = 0;

        if (varTarget != null) {
            for (DBSPExpression field: expression.fields) {
                JITType jitType = this.convertType(field.getType());
                if (jitType.is(JITUnitType.class)) {
                    // No code needs to be generated for unit values
                    index++;
                    continue;
                }

                // The variable was declared as an Uninit instruction, and we write
                // to fields of that uninit instruction.
                JITRowType tupleTypeId = this.typeCatalog.convertTupleType(expression.getType(), this.jitVisitor);
                JITScalarType fieldType = jitType.to(JITScalarType.class);
                JITInstructionPair fieldId = this.accept(field);
                VariableAssignmentTarget v = target.to(VariableAssignmentTarget.class);
                this.add(new JITStoreInstruction(this.nextInstructionId(),
                        v.storage.value, tupleTypeId, index, fieldId.value,
                        fieldType, "into " + expression + "." + index));
                if (fieldId.hasNull()) {
                    this.add(new JITSetNullInstruction(this.nextInstructionId(),
                            v.storage.value, tupleTypeId, index, fieldId.isNull));
                }
                index++;
            }
        } else {
            if (paramsTarget == null)
                throw new InternalCompilerError("Unexpected target of assignment " + target);

            // We are returning from the function.
            // We have to treat this code as if it is assigning to the
            // 'out' parameters of the function.
            // However, the out parameters may have been flattened...
            if (expression.type.is(DBSPTypeTuple.class)) {
                // If the return result of the function is a Tuple,
                // there should be exactly one out parameter with the same Tuple type.
                if (paramsTarget.size() != 1)
                    throw new InternalCompilerError("Expected just one output parameter not " + paramsTarget.size());
                VariableAssignmentTarget v = paramsTarget.getField(index).to(VariableAssignmentTarget.class);
                this.pushVariable(v);
                expression.accept(this);
                this.popVariable(v);
            } else {
                // If the return result of the function is a RawTuple,
                // then it has been "flattened" into multiple out parameters,
                // one for each field of the tuple.
                for (DBSPExpression field : expression.fields) {
                    JITType jitType = this.convertType(field.getType());
                    if (jitType.is(JITUnitType.class)) {
                        // No code needs to be generated for unit values
                        index++;
                        continue;
                    }

                    VariableAssignmentTarget v = paramsTarget.getField(index).to(VariableAssignmentTarget.class);
                    if (jitType.isScalarType()) {
                        // The out param is treated as a tuple with 1 element
                        JITInstructionPair fieldId = this.accept(field);
                        JITRowType varType = v.type.to(JITRowType.class);
                        this.add(new JITStoreInstruction(this.nextInstructionId(),
                                v.storage.value, varType, 0, fieldId.value,
                                jitType.to(JITScalarType.class), "into " + expression + "." + index));
                        if (fieldId.hasNull()) {
                            this.add(new JITSetNullInstruction(this.nextInstructionId(),
                                    v.storage.value, varType, 0, fieldId.isNull));
                        }
                    } else {
                        // Process this as a separate assignment to each out parameter.
                        // This handles nested tuples.  Nested tuples can only
                        // occur in the last expression in the generated code, where
                        // we may "return" something like ( (a), Tuple2::new(x, y) ).
                        this.pushVariable(v);
                        field.accept(this);
                        this.popVariable(v);
                    }
                    index++;
                }
            }
        }
        return VisitDecision.STOP;
    }

    JITBlock newBlock() {
        int blockId = this.blocks.size() + 1;
        JITBlock result = new JITBlock(blockId);
        this.blocks.add(result);
        return result;
    }

    void setCurrentBlock(@Nullable JITBlock block) {
        this.currentBlock = block;
    }

    @Override
    public VisitDecision preorder(DBSPBlockExpression expression) {
        this.newContext();
        JITBlock saveBlock = this.currentBlock;
        JITBlock newBlock = this.newBlock();
        if (this.currentBlock != null) {
            JITJumpTerminator terminator = new JITJumpTerminator(newBlock.createDestination());
            this.currentBlock.terminate(terminator);
        }
        this.setCurrentBlock(newBlock);
        for (DBSPStatement stat: expression.contents)
            stat.accept(this);

        // TODO: handle nullability
        JITInstructionPair resultId = new JITInstructionPair(JITInstructionRef.INVALID);
        if (expression.lastExpression != null) {
            if (this.jitVisitor.isScalarType(expression.lastExpression.getType())) {
                resultId = this.accept(expression.lastExpression);
                // Otherwise the result of the block
                // is the result computed by the last expression
                JITInstructionPair id = this.getExpressionValues(expression.lastExpression);
                this.map(expression, id);
            } else {
                // If the result is a tuple, this will store the result in the return value
                // and this block will return Unit.
                expression.lastExpression.accept(this);
            }
        }

        this.popContext();
        if (saveBlock == null) {
            // No outer block, we are terminating a closure, return.
            JITReturnTerminator ret = new JITReturnTerminator(resultId.value);
            this.getCurrentBlock().terminate(ret);
        } else {
            // Create a continuation block and jump to it.
            newBlock = newBlock();
            JITJumpTerminator terminator = new JITJumpTerminator(newBlock.createDestination());
            this.getCurrentBlock().terminate(terminator);
            this.setCurrentBlock(newBlock);
        }
        return VisitDecision.STOP;
    }

    /**
     * Convert the body of a closure expression to JIT representation.
     * @param expression  Expression to generate code for.
     * @param parent      The JIT visitor which invokes this visitor.
     * @param parameterMapping  Mapping that describes the function parameters.
     * @param catalog     The catalog of Tuple types.
     */
    static List<JITBlock> convertClosure(
            IErrorReporter reporter, ToJitVisitor parent, JITParameterMapping parameterMapping,
            DBSPClosureExpression expression, TypeCatalog catalog) {
        List<JITBlock> blocks = new ArrayList<>();
        ToJitInnerVisitor visitor = new ToJitInnerVisitor(reporter, blocks, catalog, parent, parameterMapping);
        visitor.newContext();
        expression.accept(visitor);
        visitor.popContext();
        return blocks;
    }
}
