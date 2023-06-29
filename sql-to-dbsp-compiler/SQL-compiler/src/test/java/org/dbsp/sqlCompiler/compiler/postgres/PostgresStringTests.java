package org.dbsp.sqlCompiler.compiler.postgres;

import org.dbsp.sqlCompiler.compiler.backend.DBSPCompiler;
import org.junit.Assert;
import org.junit.Ignore;
import org.junit.Test;

/**
 * https://github.com/postgres/postgres/blob/master/src/test/regress/expected/strings.out
 */
@SuppressWarnings("JavadocLinkAsPlainText")
public class PostgresStringTests extends PostgresBaseTest {
    @Override
    public void prepareData(DBSPCompiler compiler) {
        String data =
                "CREATE TABLE CHAR_TBL(f1 char(4));\n" +
                "INSERT INTO CHAR_TBL (f1) VALUES\n" +
                "  ('a'),\n" +
                "  ('ab'),\n" +
                "  ('abcd'),\n" +
                "  ('abcd    ');" +
                "CREATE TABLE VARCHAR_TBL(f1 varchar(4));\n" +
                "INSERT INTO VARCHAR_TBL (f1) VALUES\n" +
                "  ('a'),\n" +
                "  ('ab'),\n" +
                "  ('abcd'),\n" +
                "  ('abcd    ');\n" +
                "CREATE TABLE UVARCHAR_TBL(f1 varchar);\n" +
                "INSERT INTO UVARCHAR_TBL (f1) VALUES\n" +
                "  ('a'),\n" +
                "  ('ab'),\n" +
                "  ('abcd'),\n" +
                "  ('abcd    ');\n" +
                "CREATE TABLE TEXT_TBL (f1 text);\n" +
                "INSERT INTO TEXT_TBL VALUES\n" +
                "  ('doh!'),\n" +
                "  ('hi de ho neighbor');";
        compiler.compileStatements(data);
    }

    @Test
    public void continuationTest() {
        this.queryWithOutput("SELECT 'first line'\n" +
                "' - next line'\n" +
                "\t' - third line'\n" +
                "\tAS \"Three lines to one\";\n" +
                "         Three lines to one          \n" +
                "-------------------------------------\n" +
                "first line - next line - third line");
    }

    @Test
    public void illegalContinuationTest() {
        Exception exception = Assert.assertThrows(RuntimeException.class, () -> {
            // Cannot continue a string without a newline
            this.queryWithOutput("SELECT 'first line' " +
                    "' - next line'\n" +
                    "\tAS \"Illegal comment within continuation\";\n");
        });
        Assert.assertTrue(exception.getMessage().contains("String literal continued on same line"));
    }

    @Test
    public void unicodeIdentifierTest() {
        // Calcite does not support 6-digits escapes like Postgres, so
        // I have modified +000061 to just 0061
        this.queryWithOutput("SELECT U&'d\\0061t\\0061' AS U&\"d\\0061t\\0061\";\n" +
                " data \n" +
                "------\n" +
                "data");
    }

    @Test
    public void unicodeNewEscapeTest() {
        this.queryWithOutput(
                "SELECT U&'d!0061t!0061' UESCAPE '!' AS U&\"d*0061t\\0061\" UESCAPE '*';\n" +
                " data \n" +
                "------\n" +
                "data");
    }

    @Test
    public void namesWithSlashTest() {
        this.queryWithOutput("SELECT U&'a\\\\b' AS \"a\\b\";\n" +
                " a\\b \n" +
                "-----\n" +
                "a\\b");
    }

    @Test
    public void backslashWithSpacesTest() {
        this.queryWithOutput("SELECT U&' \\' UESCAPE '!' AS \"tricky\";\n" +
                "tricky \n" +
                "--------\n" +
                " \\");
    }

    @Test
    public void invalidUnicodeTest() {
        Exception exception = Assert.assertThrows(
                RuntimeException.class, () -> this.queryWithOutput("SELECT U&'wrong: \\061';\n"));
        Assert.assertTrue(exception.getMessage().contains(
                "Unicode escape sequence starting at character 7 is not exactly four hex digits"));
    }

    // Lots of other escaping tests skipped, many using the E escaping notation from Postgres

    @Test
    public void testCharN() {
        this.queryWithOutput("SELECT CAST(f1 AS text) AS \"text(char)\" FROM CHAR_TBL;\n" +
                " text(char) \n" +
                "------------\n" +
                "a\n" +
                "ab\n" +
                "abcd\n" +
                "abcd");
    }

    @Test
    public void testVarcharN() {
        this.queryWithOutput("SELECT CAST(f1 AS text) AS \"text(varchar)\" FROM VARCHAR_TBL;\n" +
                " text(varchar) \n" +
                "------------\n" +
                "a\n" +
                "ab\n" +
                "abcd\n" +
                "abcd");
    }

    @Test
    public void testVarchar() {
        this.queryWithOutput("SELECT f1 AS \"text(varchar)\" FROM UVARCHAR_TBL;\n" +
                " text(varchar) \n" +
                        "------------\n" +
                        "a\n" +
                        "ab\n" +
                        "abcd\n" +
                        "abcd");
    }

    // 'name' not supported
    // SELECT CAST(name 'namefield' AS text) AS "text(name)"

    @Test
    public void testTextTbl() {
        this.queryWithOutput("SELECT CAST(f1 AS char(10)) AS \"char(text)\" FROM TEXT_TBL;\n" +
                "char(text) \n" +
                "------------\n" +
                "doh!      \n" +
                "hi de ho n");
    }

    @Test
    public void testWiderChar() {
        this.queryWithOutput("SELECT CAST(f1 AS char(20)) AS \"char(text)\" FROM TEXT_TBL;\n" +
                "      char(text)      \n" +
                "----------------------\n" +
                "doh!                \n" +
                "hi de ho neighbor   ");
    }

    @Test
    public void testWiderVarchar() {
        this.queryWithOutput("SELECT CAST(f1 AS char(10)) AS \"char(varchar)\" FROM VARCHAR_TBL;\n" +
                " char(varchar) \n" +
                "---------------\n" +
                "a         \n" +
                "ab        \n" +
                "abcd      \n" +
                "abcd      ");
    }

    @Test
    public void testTextTbl2() {
        this.queryWithOutput("SELECT CAST(f1 AS varchar) AS \"varchar(text)\" FROM TEXT_TBL;\n" +
                "   varchar(text)   \n" +
                "-------------------\n" +
                "doh!\n" +
                "hi de ho neighbor");
    }

    @Test
    public void testCharTbl() {
        this.queryWithOutput("SELECT CAST(f1 AS varchar) AS \"varchar(char)\" FROM CHAR_TBL;\n" +
                " varchar(char) \n" +
                "---------------\n" +
                "a\n" +
                "ab\n" +
                "abcd\n" +
                "abcd");
    }

    @Test
    public void testTrimConstant() {
        this.queryWithOutput("SELECT TRIM(BOTH FROM '  bunch o blanks  ') = 'bunch o blanks' AS \"bunch o blanks\";\n" +
                " bunch o blanks \n" +
                "----------------\n" +
                " t");
        this.queryWithOutput("SELECT TRIM(LEADING FROM '  bunch o blanks  ') = 'bunch o blanks  ' AS \"bunch o blanks  \";\n" +
                " bunch o blanks   \n" +
                "------------------\n" +
                " t");
        this.queryWithOutput("SELECT TRIM(TRAILING FROM '  bunch o blanks  ') = '  bunch o blanks' AS \"  bunch o blanks\";\n" +
                "   bunch o blanks \n" +
                "------------------\n" +
                " t");
    }

    @Test
    public void testTrim() {
        this.queryWithOutput("SELECT TRIM(BOTH FROM '  bunch o blanks  ') = 'bunch o blanks' AS \"bunch o blanks\";\n" +
                " bunch o blanks \n" +
                "----------------\n" +
                " t");
        this.queryWithOutput("SELECT TRIM(LEADING FROM '  bunch o blanks  ') = 'bunch o blanks  ' AS \"bunch o blanks  \";\n" +
                " bunch o blanks   \n" +
                "------------------\n" +
                " t");
        this.queryWithOutput("SELECT TRIM(TRAILING FROM '  bunch o blanks  ') = '  bunch o blanks' AS \"  bunch o blanks\";\n" +
                "   bunch o blanks \n" +
                "------------------\n" +
                " t");
    }

    @Test
    public void testTrimArg() {
        this.queryWithOutput("SELECT TRIM(BOTH 'x' FROM 'xxxxxsome Xsxxxxx') = 'some Xs' AS \"some Xs\";\n" +
                " some Xs \n" +
                "---------\n" +
                " t");
    }

    @Test
    public void testSubstring() {
        this.queryWithOutput("SELECT SUBSTRING('1234567890' FROM 3) = '34567890' AS \"34567890\";\n" +
                " 34567890 \n" +
                "----------\n" +
                " t\n");
        this.queryWithOutput(
                "SELECT SUBSTRING('1234567890' FROM 4 FOR 3) = '456' AS \"456\";\n" +
                " 456 \n" +
                "-----\n" +
                " t\n");
        this.queryWithOutput("SELECT SUBSTRING('string' FROM -10 FOR 2147483646) AS \"string\";\n" +
                " string \n" +
                "--------\n" +
                "string");
    }

    @Test @Ignore("https://issues.apache.org/jira/browse/CALCITE-5810")
    public void testSubstringOverflow() {
        this.queryWithOutput(
                "SELECT SUBSTRING('string' FROM 2 FOR 2147483646) AS \"tring\";\n" +
                        " tring \n" +
                        "-------\n" +
                        "tring\n");
    }
}