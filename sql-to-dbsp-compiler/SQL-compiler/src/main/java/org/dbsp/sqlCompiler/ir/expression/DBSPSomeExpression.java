package org.dbsp.sqlCompiler.ir.expression;

import org.dbsp.sqlCompiler.ir.IDBSPNode;
import org.dbsp.sqlCompiler.compiler.visitors.inner.InnerVisitor;
import org.dbsp.sqlCompiler.ir.NonCoreIR;
import org.dbsp.util.IIndentStream;

import javax.annotation.Nullable;

/**
 * Represents an expression of the form Some(e).
 */
@NonCoreIR
public class DBSPSomeExpression extends DBSPExpression {
    public final DBSPExpression expression;

    public DBSPSomeExpression(@Nullable Object object, DBSPExpression expression) {
        super(object, expression.getType().setMayBeNull(true));
        this.expression = expression;
    }

    @Override
    public void accept(InnerVisitor visitor) {
        if (visitor.preorder(this).stop()) return;
        visitor.push(this);
        this.expression.accept(visitor);
        visitor.pop(this);
        visitor.postorder(this);
    }


    @Override
    public boolean sameFields(IDBSPNode other) {
        DBSPSomeExpression o = other.as(DBSPSomeExpression.class);
        if (o == null)
            return false;
        return this.expression == o.expression &&
                this.hasSameType(o);
    }

    @Override
    public IIndentStream toString(IIndentStream builder) {
        return builder.append("Some(")
                .append(this.expression)
                .append(")");
    }
}
