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
        this.q("SELECT 'first line'\n" +
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
            this.q("SELECT 'first line' " +
                    "' - next line'\n" +
                    "\tAS \"Illegal comment within continuation\";\n");
        });
        Assert.assertTrue(exception.getMessage().contains("String literal continued on same line"));
    }

    @Test
    public void unicodeIdentifierTest() {
        // Calcite does not support 6-digits escapes like Postgres, so
        // I have modified +000061 to just 0061
        this.q("SELECT U&'d\\0061t\\0061' AS U&\"d\\0061t\\0061\";\n" +
                " data \n" +
                "------\n" +
                "data");
    }

    @Test
    public void unicodeNewEscapeTest() {
        this.q(
                "SELECT U&'d!0061t!0061' UESCAPE '!' AS U&\"d*0061t\\0061\" UESCAPE '*';\n" +
                " data \n" +
                "------\n" +
                "data");
    }

    @Test
    public void namesWithSlashTest() {
        this.q("SELECT U&'a\\\\b' AS \"a\\b\";\n" +
                " a\\b \n" +
                "-----\n" +
                "a\\b");
    }

    @Test
    public void backslashWithSpacesTest() {
        this.q("SELECT U&' \\' UESCAPE '!' AS \"tricky\";\n" +
                "tricky \n" +
                "--------\n" +
                " \\");
    }

    @Test
    public void invalidUnicodeTest() {
        Exception exception = Assert.assertThrows(
                RuntimeException.class, () -> this.q("SELECT U&'wrong: \\061';\n"));
        Assert.assertTrue(exception.getMessage().contains(
                "Unicode escape sequence starting at character 7 is not exactly four hex digits"));
    }

    // Lots of other escaping tests skipped, many using the E escaping notation from Postgres

    @Test
    public void testCharN() {
        this.q("SELECT CAST(f1 AS text) AS \"text(char)\" FROM CHAR_TBL;\n" +
                " text(char) \n" +
                "------------\n" +
                "a\n" +
                "ab\n" +
                "abcd\n" +
                "abcd");
    }

    @Test
    public void testVarcharN() {
        this.q("SELECT CAST(f1 AS text) AS \"text(varchar)\" FROM VARCHAR_TBL;\n" +
                " text(varchar) \n" +
                "------------\n" +
                "a\n" +
                "ab\n" +
                "abcd\n" +
                "abcd");
    }

    @Test
    public void testVarchar() {
        this.q("SELECT f1 AS \"text(varchar)\" FROM UVARCHAR_TBL;\n" +
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
        this.q("SELECT CAST(f1 AS char(10)) AS \"char(text)\" FROM TEXT_TBL;\n" +
                "char(text) \n" +
                "------------\n" +
                "doh!      \n" +
                "hi de ho n");
    }

    @Test
    public void testWiderChar() {
        this.q("SELECT CAST(f1 AS char(20)) AS \"char(text)\" FROM TEXT_TBL;\n" +
                "      char(text)      \n" +
                "----------------------\n" +
                "doh!                \n" +
                "hi de ho neighbor   ");
    }

    @Test
    public void testWiderVarchar() {
        this.q("SELECT CAST(f1 AS char(10)) AS \"char(varchar)\" FROM VARCHAR_TBL;\n" +
                " char(varchar) \n" +
                "---------------\n" +
                "a         \n" +
                "ab        \n" +
                "abcd      \n" +
                "abcd      ");
    }

    @Test
    public void testTextTbl2() {
        this.q("SELECT CAST(f1 AS varchar) AS \"varchar(text)\" FROM TEXT_TBL;\n" +
                "   varchar(text)   \n" +
                "-------------------\n" +
                "doh!\n" +
                "hi de ho neighbor");
    }

    @Test
    public void testCharTbl() {
        this.q("SELECT CAST(f1 AS varchar) AS \"varchar(char)\" FROM CHAR_TBL;\n" +
                " varchar(char) \n" +
                "---------------\n" +
                "a\n" +
                "ab\n" +
                "abcd\n" +
                "abcd");
    }

    @Test
    public void testTrimConstant() {
        this.q("SELECT TRIM(BOTH FROM '  bunch o blanks  ') = 'bunch o blanks' AS \"bunch o blanks\";\n" +
                " bunch o blanks \n" +
                "----------------\n" +
                " t");
        this.q("SELECT TRIM(LEADING FROM '  bunch o blanks  ') = 'bunch o blanks  ' AS \"bunch o blanks  \";\n" +
                " bunch o blanks   \n" +
                "------------------\n" +
                " t");
        this.q("SELECT TRIM(TRAILING FROM '  bunch o blanks  ') = '  bunch o blanks' AS \"  bunch o blanks\";\n" +
                "   bunch o blanks \n" +
                "------------------\n" +
                " t");
    }

    @Test
    public void testTrim() {
        this.q("SELECT TRIM(BOTH FROM '  bunch o blanks  ') = 'bunch o blanks' AS \"bunch o blanks\";\n" +
                " bunch o blanks \n" +
                "----------------\n" +
                " t");
        this.q("SELECT TRIM(LEADING FROM '  bunch o blanks  ') = 'bunch o blanks  ' AS \"bunch o blanks  \";\n" +
                " bunch o blanks   \n" +
                "------------------\n" +
                " t");
        this.q("SELECT TRIM(TRAILING FROM '  bunch o blanks  ') = '  bunch o blanks' AS \"  bunch o blanks\";\n" +
                "   bunch o blanks \n" +
                "------------------\n" +
                " t");
    }

    @Test
    public void testTrimArg() {
        this.q("SELECT TRIM(BOTH 'x' FROM 'xxxxxsome Xsxxxxx') = 'some Xs' AS \"some Xs\";\n" +
                " some Xs \n" +
                "---------\n" +
                " t");
    }

    @Test
    public void testSubstring() {
        this.q("SELECT SUBSTRING('1234567890' FROM 3) = '34567890' AS \"34567890\";\n" +
                " 34567890 \n" +
                "----------\n" +
                " t");
        this.q(
                "SELECT SUBSTRING('1234567890' FROM 4 FOR 3) = '456' AS \"456\";\n" +
                " 456 \n" +
                "-----\n" +
                " t");
        this.q("SELECT SUBSTRING('string' FROM -10 FOR 2147483646) AS \"string\";\n" +
                " string \n" +
                "--------\n" +
                "string");
    }

    @Test @Ignore("https://issues.apache.org/jira/browse/CALCITE-5810")
    public void testSubstringOverflow() {
        this.q(
                "SELECT SUBSTRING('string' FROM 2 FOR 2147483646) AS \"tring\";\n" +
                        " tring \n" +
                        "-------\n" +
                        "tring");
    }

    @Test
    public void testNegativeSubstringLength() {
        this.q("SELECT SUBSTRING('string' FROM -10 FOR -2147483646) AS \"error\";\n" +
                "error\n" +
                "------\n" +
                "");
        this.q("SELECT SUBSTRING('string' FROM 0 FOR -2) AS \"error\";\n" +
                "error\n" +
                "------\n" +
                "");
    }

    @Test
    public void testPosition() {
        this.q("SELECT POSITION('4' IN '1234567890') = '4' AS \"4\";\n" +
                " 4 \n" +
                "---\n" +
                " t");
        this.q("SELECT POSITION('5' IN '1234567890') = '5' AS \"5\";\n" +
                " 5 \n" +
                "---\n" +
                " t");
        this.q("SELECT POSITION('A' IN '1234567890') = '0' AS \"0\";\n" +
                " 5 \n" +
                "---\n" +
                " t");
    }

    // SUBSTRING ... SIMILAR syntax not supported
    // SELECT SUBSTRING('abcdefg' SIMILAR 'a#"(b_d)#"%' ESCAPE '#') AS "bcd";

    @Test
    public void testOverlay() {
        this.q("SELECT OVERLAY('abcdef' PLACING '45' FROM 4) AS \"abc45f\";\n" +
                " abc45f \n" +
                "--------\n" +
                "abc45f");
        this.q("SELECT OVERLAY('yabadoo' PLACING 'daba' FROM 5) AS \"yabadaba\";\n" +
                " yabadaba \n" +
                "----------\n" +
                "yabadaba");
        this.q("SELECT OVERLAY('yabadoo' PLACING 'daba' FROM 5 FOR 0) AS \"yabadabadoo\";\n" +
                " yabadabadoo \n" +
                "-------------\n" +
                "yabadabadoo");
        this.q("SELECT OVERLAY('babosa' PLACING 'ubb' FROM 2 FOR 4) AS \"bubba\";\n" +
                " bubba \n" +
                "-------\n" +
                "bubba");
    }

    @Test @Ignore("Not yet implemented")
    public void testRegexpReplace() {
        this.q("SELECT regexp_replace('1112223333', E'(\\\\d{3})(\\\\d{3})(\\\\d{4})', E'(\\\\1) \\\\2-\\\\3');\n" +
                " regexp_replace \n" +
                "----------------\n" +
                "(111) 222-3333");
        this.q("SELECT regexp_replace('foobarrbazz', E'(.)\\\\1', E'X\\\\&Y', 'g');\n" +
                "  regexp_replace   \n" +
                "-------------------\n" +
                "fXooYbaXrrYbaXzzY");
        this.q("SELECT regexp_replace('foobarrbazz', E'(.)\\\\1', E'X\\\\\\\\Y', 'g');\n" +
                " regexp_replace \n" +
                "----------------\n" +
                "fX\\YbaX\\YbaX\\Y");
        this.q("SELECT regexp_replace('foobarrbazz', E'(.)\\\\1', E'X\\\\Y\\\\1Z\\\\');\n" +
                " regexp_replace  \n" +
                "-----------------\n" +
                "fX\\YoZ\\barrbazz");
        this.q("SELECT regexp_replace('AAA   BBB   CCC   ', E'\\\\s+', ' ', 'g');\n" +
                " regexp_replace \n" +
                "----------------\n" +
                "AAA BBB CCC ");
        this.q("SELECT regexp_replace('AAA', '^|$', 'Z', 'g');\n" +
                " regexp_replace \n" +
                "----------------\n" +
                " ZAAAZ");
        this.q("SELECT regexp_replace('AAA aaa', 'A+', 'Z', 'gi');\n" +
                " regexp_replace \n" +
                "----------------\n" +
                " Z Z");
        this.q(
                "-- invalid regexp option\n" +
                "SELECT regexp_replace('AAA aaa', 'A+', 'Z', 'z');\n" +
                "ERROR:  invalid regular expression option: \"z\"");
        this.q("SELECT regexp_replace('A PostgreSQL function', 'A|e|i|o|u', 'X', 1);\n" +
                "    regexp_replace     \n" +
                "-----------------------\n" +
                " X PostgreSQL function");
        this.q("SELECT regexp_replace('A PostgreSQL function', 'A|e|i|o|u', 'X', 1, 2);\n" +
                "    regexp_replace     \n" +
                "-----------------------\n" +
                " A PXstgreSQL function");
        this.q("SELECT regexp_replace('A PostgreSQL function', 'a|e|i|o|u', 'X', 1, 0, 'i');\n" +
                "    regexp_replace     \n" +
                "-----------------------\n" +
                " X PXstgrXSQL fXnctXXn");
        this.q("SELECT regexp_replace('A PostgreSQL function', 'a|e|i|o|u', 'X', 1, 1, 'i');\n" +
                "    regexp_replace     \n" +
                "-----------------------\n" +
                " X PostgreSQL function");
        this.q("SELECT regexp_replace('A PostgreSQL function', 'a|e|i|o|u', 'X', 1, 2, 'i');\n" +
                "    regexp_replace     \n" +
                "-----------------------\n" +
                " A PXstgreSQL function");
        this.q("SELECT regexp_replace('A PostgreSQL function', 'a|e|i|o|u', 'X', 1, 3, 'i');\n" +
                "    regexp_replace     \n" +
                "-----------------------\n" +
                " A PostgrXSQL function");
        this.q("SELECT regexp_replace('A PostgreSQL function', 'a|e|i|o|u', 'X', 1, 9, 'i');\n" +
                "    regexp_replace     \n" +
                "-----------------------\n" +
                " A PostgreSQL function");
        this.q("SELECT regexp_replace('A PostgreSQL function', 'A|e|i|o|u', 'X', 7, 0, 'i');\n" +
                "    regexp_replace     \n" +
                "-----------------------\n" +
                " A PostgrXSQL fXnctXXn");
        this.q("-- 'g' flag should be ignored when N is specified\n" +
                "SELECT regexp_replace('A PostgreSQL function', 'a|e|i|o|u', 'X', 1, 1, 'g');\n" +
                "    regexp_replace     \n" +
                "-----------------------\n" +
                " A PXstgreSQL function");
    }

    // TODO: regexp_count
    // TODO: regexp_like
    // TODO: regexp_instr
    // TODO: regexp_substr
    // TODO: regexp_matches
    // TODO: regexp_split_to_array
    // TODO: regexp_split_to_table

    @Test
    public void testLike2() {
        this.q("SELECT 'hawkeye' LIKE 'h%' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'hawkeye' NOT LIKE 'h%' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'hawkeye' LIKE 'H%' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'hawkeye' NOT LIKE 'H%' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'hawkeye' LIKE 'indio%' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'hawkeye' NOT LIKE 'indio%' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'hawkeye' LIKE 'h%eye' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'hawkeye' NOT LIKE 'h%eye' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'indio' LIKE '_ndio' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'indio' NOT LIKE '_ndio' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'indio' LIKE 'in__o' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'indio' NOT LIKE 'in__o' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'indio' LIKE 'in_o' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'indio' NOT LIKE 'in_o' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
    }

    @Test
    public void testLike3() {
        this.q("SELECT 'hawkeye' LIKE 'h%' ESCAPE '#' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'hawkeye' NOT LIKE 'h%' ESCAPE '#' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'indio' LIKE 'ind_o' ESCAPE '$' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'indio' NOT LIKE 'ind_o' ESCAPE '$' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'h%' LIKE 'h#%' ESCAPE '#' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'h%' NOT LIKE 'h#%' ESCAPE '#' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'h%wkeye' LIKE 'h#%' ESCAPE '#' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'h%wkeye' NOT LIKE 'h#%' ESCAPE '#' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'h%wkeye' LIKE 'h#%%' ESCAPE '#' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'h%wkeye' NOT LIKE 'h#%%' ESCAPE '#' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'h%awkeye' LIKE 'h#%a%k%e' ESCAPE '#' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'h%awkeye' NOT LIKE 'h#%a%k%e' ESCAPE '#' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'indio' LIKE '_ndio' ESCAPE '$' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'indio' NOT LIKE '_ndio' ESCAPE '$' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'i_dio' LIKE 'i$_d_o' ESCAPE '$' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'i_dio' NOT LIKE 'i$_d_o' ESCAPE '$' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'i_dio' LIKE 'i$_nd_o' ESCAPE '$' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'i_dio' NOT LIKE 'i$_nd_o' ESCAPE '$' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'i_dio' LIKE 'i$_d%o' ESCAPE '$' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'i_dio' NOT LIKE 'i$_d%o' ESCAPE '$' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
    }

    @Test @Ignore("We do not allow escape characters that are % or _")
    public void testLike3Pattern() {
        // -- escape character same as pattern character\n"
        this.q("SELECT 'maca' LIKE 'm%aca' ESCAPE '%' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'maca' NOT LIKE 'm%aca' ESCAPE '%' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'ma%a' LIKE 'm%a%%a' ESCAPE '%' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'ma%a' NOT LIKE 'm%a%%a' ESCAPE '%' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'bear' LIKE 'b_ear' ESCAPE '_' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'bear' NOT LIKE 'b_ear' ESCAPE '_' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'be_r' LIKE 'b_e__r' ESCAPE '_' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
        this.q("SELECT 'be_r' NOT LIKE 'b_e__r' ESCAPE '_' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'be_r' LIKE '__e__r' ESCAPE '_' AS \"false\";\n" +
                " false \n" +
                "-------\n" +
                " f");
        this.q("SELECT 'be_r' NOT LIKE '__e__r' ESCAPE '_' AS \"true\";\n" +
                " true \n" +
                "------\n" +
                " t");
    }

    // TODO ILIKE

    @Test
    public void testLikeCombinations() {
        this.q("SELECT 'foo' LIKE '_%' as t, 'f' LIKE '_%' as t, '' LIKE '_%' as f;\n" +
                " t | t | f \n" +
                "---+---+---\n" +
                " t | t | f");
        this.q("SELECT 'foo' LIKE '%_' as t, 'f' LIKE '%_' as t, '' LIKE '%_' as f;\n" +
                " t | t | f \n" +
                "---+---+---\n" +
                " t | t | f");
        this.q("SELECT 'foo' LIKE '__%' as t, 'foo' LIKE '___%' as t, 'foo' LIKE '____%' as f;\n" +
                " t | t | f \n" +
                "---+---+---\n" +
                " t | t | f");
        this.q("SELECT 'foo' LIKE '%__' as t, 'foo' LIKE '%___' as t, 'foo' LIKE '%____' as f;\n" +
                " t | t | f \n" +
                "---+---+---\n" +
                " t | t | f");
        this.q("SELECT 'jack' LIKE '%____%' AS t;\n" +
                " t \n" +
                "---\n" +
                " t");
    }

    @Test
    public void testConcatConversions() {
        // In Postgres concatenation converts to text, whereas Calcite does not.
        this.q("SELECT 'unknown' || ' and unknown' AS \"Concat unknown types\";\n" +
                " Concat unknown types \n" +
                "----------------------\n" +
                "unknown and unknown");
        this.q("SELECT 'text'::text || ' and unknown' AS \"Concat text to unknown type\";\n" +
                " Concat text to unknown type \n" +
                "-----------------------------\n" +
                "text and unknown");
        this.q("SELECT 'characters' ::char(20) || ' and text' AS \"Concat char to unknown type\";\n" +
                " Concat char to unknown type \n" +
                "-----------------------------\n" +
                "characters           and text");
        this.q("SELECT 'text'::text || ' and characters'::char(20) AS \"Concat text to char\";\n" +
                " Concat text to char \n" +
                "---------------------\n" +
                "text and characters     ");
        this.q("SELECT 'text'::text || ' and varchar'::varchar AS \"Concat text to varchar\";\n" +
                " Concat text to varchar \n" +
                "------------------------\n" +
                "text and varchar");
    }

    @Test
    public void testLength() {
        // length in postgres is equivalent to char_length
        this.q("SELECT char_length('abcdef') AS \"length_6\";\n" +
                " length_6 \n" +
                "----------\n" +
                "        6");
        this.q("SELECT character_length('abcdef') AS \"length_6\";\n" +
                " length_6 \n" +
                "----------\n" +
                "        6");
        this.q("SELECT character_length('josé') AS \"length\";\n" +
                " length \n" +
                "--------\n" +
                "       4");
    }

    @Test
    public void testStrpos() {
        // No strpos in Calcite, replace with 'position' with arguments swapped
        this.q("SELECT POSITION('cd' IN 'abcdef') AS \"pos_3\";\n" +
                " pos_3 \n" +
                "-------\n" +
                "     3");
        this.q("SELECT POSITION('xy' IN 'abcdef') AS \"pos_0\";\n" +
                " pos_0 \n" +
                "-------\n" +
                "     0");
        this.q("SELECT POSITION('' IN 'abcdef') AS \"pos_1\";\n" +
                " pos_1 \n" +
                "-------\n" +
                "     1");
        this.q("SELECT POSITION('xy' IN '') AS \"pos_0\";\n" +
                " pos_0 \n" +
                "-------\n" +
                "     0");
        this.q("SELECT POSITION('' IN '') AS \"pos_1\";\n" +
                " pos_1 \n" +
                "-------\n" +
                "     1");
    }

    @Test @Ignore("https://issues.apache.org/jira/browse/CALCITE-5813")
    public void testReplace() {
        this.q("SELECT REPLACE('abcdef', 'de', '45') AS \"abc45f\";\n" +
                " abc45f \n" +
                "--------\n" +
                "abc45f");
        this.q("SELECT replace('yabadabadoo', 'ba', '123') AS \"ya123da123doo\";\n" +
                " ya123da123doo \n" +
                "---------------\n" +
                "ya123da123doo");
        this.q("SELECT replace('yabadoo', 'bad', '') AS \"yaoo\";\n" +
                " yaoo \n" +
                "------\n" +
                "yaoo");
    }

    // TODO: split_part
    // TODO: to_hex
    // TODO: sha, encode, decode

    @Test
    public void testConforming() {
        this.q("select 'a\\bcd' as f1, 'a\\b''cd' as f2, 'a\\b''''cd' as f3, 'abcd\\'   as f4, 'ab\\''cd' as f5, '\\\\' as f6;\n" +
                "  f1  |  f2   |   f3   | f4   |   f5  | f6 \n" +
                "------+-------+--------+------+-------+----\n" +
                "a\\bcd|a\\b'cd|a\\b''cd|abcd\\|ab\\'cd|\\\\");
    }

    // TODO: lpat, translate,
    
    @Test
    public void testAscii() {
        this.q("SELECT ascii('x');\n" +
                " ascii \n" +
                "-------\n" +
                "   120");
        this.q("SELECT ascii('');\n" +
                " ascii \n" +
                "-------\n" +
                "     0");
    }

    @Test
    public void testChr() {
        this.q("SELECT chr(65);\n" +
                " chr \n" +
                "-----\n" +
                "A");
        this.q("SELECT chr(0);\n" +
                " chr \n" +
                "-----\n" +
                "\0");
    }

    @Test @Ignore("https://issues.apache.org/jira/browse/CALCITE-5813")
    public void testRepeat() {
        this.q("SELECT repeat('Pg', 4);\n" +
                "  repeat  \n" +
                "----------\n" +
                "PgPgPgPg");
        this.q("SELECT repeat('Pg', -4);\n" +
                " repeat \n" +
                "--------\n" +
                "");
    }

    // TODO: bytea computations
    // TODO unistr
}
