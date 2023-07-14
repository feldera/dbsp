package org.dbsp.sqlCompiler.compiler.postgres;

import org.dbsp.sqlCompiler.compiler.backend.DBSPCompiler;
import org.junit.Test;

/**
 * <a href="https://github.com/postgres/postgres/blob/master/src/test/regress/expected/collate.out">...</a>
 */
public class PostgresCollateTests extends PostgresBaseTest {
    @Override
    public void prepareData(DBSPCompiler compiler) {
        // Per-column collation not supported in Calcite
        String sql = "CREATE TABLE collate_test10 (\n" +
                "    a int,\n" +
                "    x text, -- COLLATE \"C\",\n" +
                "    y text -- COLLATE \"POSIX\"\n" +
                ");\n" +
                "INSERT INTO collate_test10 VALUES (1, 'hij', 'hij'), (2, 'HIJ', 'HIJ');";
        compiler.compileStatements(sql);
    }

    @Test
    public void testUpperLower() {
        this.q("SELECT a, lower(x), lower(y), upper(x), upper(y), initcap(x), initcap(y) FROM collate_test10;\n" +
                " a | lower | lower | upper | upper | initcap | initcap \n" +
                "---+-------+-------+-------+-------+---------+---------\n" +
                " 1 |hij|hij|HIJ|HIJ|Hij|Hij\n" +
                " 2 |hij|hij|HIJ|HIJ|Hij|Hij");
    }
}
