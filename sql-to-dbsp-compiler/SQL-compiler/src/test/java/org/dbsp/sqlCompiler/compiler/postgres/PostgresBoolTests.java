package org.dbsp.sqlCompiler.compiler.postgres;

import org.junit.Test;

/**
 * <a href="https://github.com/postgres/postgres/blob/master/src/test/regress/expected/boolean.out">boolean.out</a>
 */
public class PostgresBoolTests extends PostgresBaseTest {
    @Test
    public void testOne() {
        this.q("SELECT 1 as 'one';\n" + " one \n" +
                "-----\n" +
                "   1");
    }

    @Test
    public void testFalse() {
        this.q("SELECT true AS true;\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT false AS false;\n" +
                " false \n" +
                "-------\n" +
                " f");
    }

    // SELECT bool 't' as 'true'
    // SELECT bool '   f           ' AS false
    // SELECT bool 'true' AS true
    // SELECT bool 'test' AS error;
    // ERROR:  invalid input syntax for type boolean: "test"
    // SELECT bool 'yes' AS true
    // SELECT bool 'yeah' AS error;
    // ERROR:  invalid input syntax for type boolean: "yeah"
    // SELECT bool 'no' AS false
    // SELECT bool 'on' AS true;
    // SELECT bool 'off' AS false;
    // SELECT bool 'of' AS false;
    // SELECT bool 'o' AS error;
    // ERROR:  invalid input syntax for type boolean: "o"
    // SELECT bool 'on_' AS error;
    // ERROR:  invalid input syntax for type boolean: "on_"
    // SELECT bool 'off_' AS error;
    // ERROR:  invalid input syntax for type boolean: "off_"
    // SELECT bool '1' AS true;
    // SELECT bool '11' AS error;
    // ERROR:  invalid input syntax for type boolean: "11"
    // SELECT bool '0' AS false;
    // SELECT bool '000' AS error;
    // SELECT bool '' AS error;
    // ERROR:  invalid input syntax for type boolean: ""
}
