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

package org.dbsp.sqlCompiler.compiler.backend.jit.ir.instructions;

import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.BaseJsonNode;
import com.fasterxml.jackson.databind.node.ObjectNode;
import org.dbsp.sqlCompiler.compiler.backend.jit.ToJitVisitor;
import org.dbsp.sqlCompiler.compiler.backend.jit.ir.types.JITRowType;
import org.dbsp.sqlCompiler.ir.expression.DBSPExpression;
import org.dbsp.sqlCompiler.ir.expression.DBSPTupleExpression;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPZSetLiteral;
import org.dbsp.util.IIndentStream;

import java.util.HashMap;
import java.util.Map;

public class JITZSetLiteral extends JITValue {
    public final Map<JITTupleLiteral, Long> elements;
    public final JITRowType rowType;

    public JITZSetLiteral(DBSPZSetLiteral zset, JITRowType type, ToJitVisitor jitVisitor) {
        this.elements = new HashMap<>(zset.size());
        this.rowType = type;
        for (Map.Entry<DBSPExpression, Long> element : zset.data.data.entrySet()) {
            long weight = element.getValue();
            DBSPTupleExpression elementValue = element.getKey().to(DBSPTupleExpression.class);
            JITTupleLiteral row = new JITTupleLiteral(elementValue, jitVisitor);
            this.elements.put(row, weight);
        }
    }

    @Override
    public BaseJsonNode asJson() {
        ObjectNode result = jsonFactory().createObjectNode();
        ObjectNode layout = result.putObject("layout");
        layout.put("Set", this.rowType.getId());
        ObjectNode value = result.putObject("value");
        ArrayNode set = value.putArray("Set");
        for (Map.Entry<JITTupleLiteral, Long> element : this.elements.entrySet()) {
            long weight = element.getValue();
            ArrayNode array = set.addArray();
            BaseJsonNode keyJson = element.getKey().asJson();
            array.add(keyJson);
            array.add(weight);
        }
        return result;
    }

    @Override
    public IIndentStream toString(IIndentStream builder) {
        builder.append("{");
        boolean first = true;
        for (Map.Entry<JITTupleLiteral, Long> e: this.elements.entrySet()) {
            if (!first)
                builder.newline();
            first = false;
            builder.append(e.getKey());
            if (e.getValue() != 1) {
                builder.append(" => ")
                        .append(e.getValue());
            }
        }
        return builder.append("}");
    }
}
