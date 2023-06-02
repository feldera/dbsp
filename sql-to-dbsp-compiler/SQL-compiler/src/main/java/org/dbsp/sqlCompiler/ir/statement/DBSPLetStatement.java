/*
 * Copyright 2022 VMware, Inc.
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

package org.dbsp.sqlCompiler.ir.statement;

import org.dbsp.sqlCompiler.ir.CircuitVisitor;
import org.dbsp.sqlCompiler.ir.InnerVisitor;
import org.dbsp.sqlCompiler.circuit.IDBSPDeclaration;
import org.dbsp.sqlCompiler.ir.expression.DBSPExpression;
import org.dbsp.sqlCompiler.ir.expression.DBSPVariablePath;
import org.dbsp.sqlCompiler.ir.type.DBSPType;

import javax.annotation.Nullable;

public class DBSPLetStatement extends DBSPStatement implements IDBSPDeclaration {
    public final String variable;
    public final DBSPType type;
    @Nullable
    public final DBSPExpression initializer;
    public final boolean mutable;

    public DBSPLetStatement(String variable, DBSPExpression initializer, boolean mutable) {
        super(null);
        this.variable = variable;
        this.initializer = initializer;
        this.type = initializer.getNonVoidType();
        this.mutable = mutable;
    }

    public DBSPLetStatement(String variable, DBSPType type, boolean mutable) {
        super(null);
        this.variable = variable;
        this.initializer = null;
        this.type = type;
        this.mutable = mutable;
    }

    public DBSPLetStatement(String variable, DBSPExpression initializer) {
        this(variable, initializer, false);
    }

    @Override
    public String getName() {
        return this.variable;
    }

    public DBSPVariablePath getVarReference() {
        return this.type.var(this.variable);
    }

    @Override
    public void accept(InnerVisitor visitor) {
        if (!visitor.preorder(this)) return;
        this.type.accept(visitor);
        if (this.initializer != null)
            this.initializer.accept(visitor);
        visitor.postorder(this);
    }
}
