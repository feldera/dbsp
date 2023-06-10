package org.dbsp.sqlCompiler.compiler.visitors.outer;

import org.dbsp.sqlCompiler.circuit.DBSPCircuit;
import org.dbsp.sqlCompiler.compiler.IErrorReporter;
import org.dbsp.sqlCompiler.compiler.backend.ToDotVisitor;
import org.dbsp.sqlCompiler.compiler.errors.SourcePositionRange;
import org.dbsp.util.IModule;
import org.dbsp.util.Logger;

import java.util.function.Supplier;

/**
 * Applies a visitor until the circuit stops changing.
 */
public class RepeatVisitor extends CircuitVisitor implements IModule {
    public CircuitVisitor visitor;

    public RepeatVisitor(IErrorReporter reporter, CircuitVisitor visitor) {
        super(reporter, false);
        this.visitor = visitor;
    }

    @Override
    public DBSPCircuit apply(DBSPCircuit circuit) {
        int maxRepeats = circuit.size();
        int repeats = 0;
        while (true) {
            DBSPCircuit result = this.visitor.apply(circuit);
            Logger.INSTANCE.from(this, 3)
                    .append("After ")
                    .append(this.visitor.toString())
                    .newline()
                    .append((Supplier<String>) result::toString)
                    .newline();
            if (this.getDebugLevel() >= 3) {
                String name = this.visitor.toString().replace(" ", "_") + repeats + ".png";
                Logger.INSTANCE.from(this, 3)
                        .append("Writing circuit to ")
                        .append(name)
                        .newline();
                ToDotVisitor.toDot(this.errorReporter, name, "png", result);
            }
            if (result.sameCircuit(circuit))
                return circuit;
            circuit = result;
            repeats++;
            if (repeats == maxRepeats) {
                this.errorReporter.reportError(SourcePositionRange.INVALID, true,
                        "InfiniteLoop",
                        "Repeated optimization " + this.visitor + " " +
                        repeats + " times without convergence");
                return result;
            }
        }
    }

    @Override
    public String toString() {
        return "Repeat " + this.visitor;
    }
}
