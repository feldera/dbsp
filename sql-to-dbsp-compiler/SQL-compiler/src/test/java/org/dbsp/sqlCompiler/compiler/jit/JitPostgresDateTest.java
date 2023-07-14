package org.dbsp.sqlCompiler.compiler.jit;

import org.dbsp.sqlCompiler.compiler.CompilerOptions;
import org.dbsp.sqlCompiler.compiler.postgres.PostgresDateTests;
import org.junit.Ignore;
import org.junit.Test;

public class JitPostgresDateTest extends PostgresDateTests {
    public CompilerOptions getOptions(boolean optimize) {
        CompilerOptions options = super.getOptions(optimize);
        options.ioOptions.jit = true;
        return options;
    }

    // TODO: all ignored tests below are JIT bugs

    @Test @Ignore("No support for intervals https://github.com/feldera/dbsp/issues/309")
    public void testDiff() {
        this.q("SELECT (f1 - date '2000-01-01') day AS \"Days From 2K\" FROM DATE_TBL;\n" +
                " Days From 2K \n" +
                "--------------\n" +
                "       -15607\n" +
                "       -15542\n" +
                "        -1403\n" +
                "        -1402\n" +
                "        -1401\n" +
                "        -1400\n" +
                "        -1037\n" +
                "        -1036\n" +
                "        -1035\n" +
                "           91\n" +
                "           92\n" +
                "           93\n" +
                "        13977\n" +
                "        14343\n" +
                "        14710\n" +
                "null"); // Added manually
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testLt() {
        this.q("SELECT f1 FROM DATE_TBL WHERE f1 < '2000-01-01';\n" +
                " f1       \n" +
                "---------------\n" +
                " 04-09-1957\n" +
                " 06-13-1957\n" +
                " 02-28-1996\n" +
                " 02-29-1996\n" +
                " 03-01-1996\n" +
                " 03-02-1996\n" +
                " 02-28-1997\n" +
                " 03-01-1997\n" +
                " 03-02-1997");
        //" 04-10-2040 BC";
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testBetween() {
        this.q("SELECT f1 FROM DATE_TBL\n" +
                "  WHERE f1 BETWEEN '2000-01-01' AND '2001-01-01';\n" +
                " f1\n" +
                "----------------\n" +
                "04-01-2000\n" +
                "04-02-2000\n" +
                "04-03-2000");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testEpoch() {
        this.q("SELECT EXTRACT(EPOCH FROM DATE '1970-01-01');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testCentury() {
        this.q("SELECT EXTRACT(CENTURY FROM DATE '0001-01-01');\n" +
                " extract \n" +
                "---------\n" +
                "       1");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testCentury1() {
        this.q("SELECT EXTRACT(CENTURY FROM DATE '1900-12-31');\n" +
                " extract \n" +
                "---------\n" +
                "      19");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testCentury2() {
        this.q("SELECT EXTRACT(CENTURY FROM DATE '1901-01-01');\n" +
                " extract \n" +
                "---------\n" +
                "      20");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testCentury3() {
        this.q("SELECT EXTRACT(CENTURY FROM DATE '2000-12-31');\n" +
                " extract \n" +
                "---------\n" +
                "      20");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testCentury4() {
        this.q("SELECT EXTRACT(CENTURY FROM DATE '2001-01-01');\n" +
                " extract \n" +
                "---------\n" +
                "      21");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testMillennium() {
        this.q("SELECT EXTRACT(MILLENNIUM FROM DATE '0001-01-01');\n" +
                " extract \n" +
                "---------\n" +
                "       1");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testMillennium1() {
        this.q("SELECT EXTRACT(MILLENNIUM FROM DATE '1000-12-31');\n" +
                " extract \n" +
                "---------\n" +
                "       1");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testMillennium2() {
        this.q("SELECT EXTRACT(MILLENNIUM FROM DATE '2000-12-31');\n" +
                " extract \n" +
                "---------\n" +
                "       2");
    }

    // CURRENT_DATE not supported
    // SELECT EXTRACT(CENTURY FROM CURRENT_DATE)>=21 AS True;     -- true

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testMillennium3() {
        this.q("SELECT EXTRACT(MILLENNIUM FROM DATE '2001-01-01');\n" +
                " extract \n" +
                "---------\n" +
                "       3");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testDecade() {
        this.q("SELECT EXTRACT(DECADE FROM DATE '1994-12-25');\n" +
                " extract \n" +
                "---------\n" +
                "     199");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testDecade1() {
        this.q("SELECT EXTRACT(DECADE FROM DATE '0010-01-01');\n" +
                " extract \n" +
                "---------\n" +
                "       1");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testDecade2() {
        this.q("SELECT EXTRACT(DECADE FROM DATE '0009-12-31');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testMicroseconds() {
        this.q("SELECT EXTRACT(MICROSECOND  FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    // These fail in Postgres but work in Calcite
    // SELECT EXTRACT(MICROSECONDS  FROM DATE '2020-08-11');
    //ERROR:  unit "microseconds" not supported for type date
    //SELECT EXTRACT(MILLISECONDS  FROM DATE '2020-08-11');
    //ERROR:  unit "milliseconds" not supported for type date
    //SELECT EXTRACT(SECOND        FROM DATE '2020-08-11');
    //ERROR:  unit "second" not supported for type date
    //SELECT EXTRACT(MINUTE        FROM DATE '2020-08-11');
    //ERROR:  unit "minute" not supported for type date
    //SELECT EXTRACT(HOUR          FROM DATE '2020-08-11');
    //ERROR:  unit "hour" not supported for type date

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testMilliseconds() {
        this.q("SELECT EXTRACT(MILLISECOND  FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testSeconds() {
        this.q("SELECT EXTRACT(SECOND        FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testSeconds0() {
        this.q("SELECT SECOND(DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testMinutes() {
        this.q("SELECT EXTRACT(MINUTE FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testMinutes1() {
        this.q("SELECT MINUTE(DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testHour() {
        this.q("SELECT EXTRACT(HOUR          FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testHour1() {
        this.q("SELECT HOUR(DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testDay() {
        this.q("SELECT EXTRACT(DAY           FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       11");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testDay1() {
        this.q("SELECT DAYOFMONTH(DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       11");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testMonth() {
        this.q("SELECT EXTRACT(MONTH         FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       8");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testMonth1() {
        this.q("SELECT MONTH(DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       8");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testYear() {
        this.q("SELECT EXTRACT(YEAR          FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     2020");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testYear1() {
        this.q("SELECT YEAR(DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     2020");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testDecade5() {
        this.q("SELECT EXTRACT(DECADE        FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     202");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testCentury5() {
        this.q("SELECT EXTRACT(CENTURY       FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     21");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testMillennium5() {
        this.q("SELECT EXTRACT(MILLENNIUM    FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     3");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testIsoYear() {
        this.q("SELECT EXTRACT(ISOYEAR       FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     2020");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testQuarter() {
        this.q("SELECT EXTRACT(QUARTER       FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     3");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testWeek() {
        this.q("SELECT EXTRACT(WEEK          FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     33");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testDow() {
        this.q("SELECT EXTRACT(DOW           FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     3");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testDow2() {
        this.q("SELECT DAYOFWEEK(DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     3");
    }

    /*
     * TODO Postgres actually returns 0 for this query, but the Calcite optimizer
     * folds this to 1. Same for previous test.
     */
    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testDow1() {
        // Sunday
        this.q("SELECT EXTRACT(DOW FROM DATE '2020-08-16');\n" +
                " extract \n" +
                "---------\n" +
                "     1");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testIsoDow() {
        this.q("SELECT EXTRACT(ISODOW FROM DATE '2020-08-16');\n" +
                " extract \n" +
                "---------\n" +
                "     7");
    }

    @Test @Ignore("No support for date constants https://github.com/feldera/dbsp/issues/340")
    public void testDoy() {
        this.q("SELECT EXTRACT(DOY           FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     224");
    }

    @Test @Ignore("Overflow in epoch https://github.com/feldera/dbsp/issues/342")
    public void testExtract() {
        this.q(
                "SELECT f1 as \"date\",\n" +
                "    EXTRACT(YEAR from f1) AS 'year',\n" +
                "    EXTRACT(month from f1) AS 'month',\n" +
                "    EXTRACT(day from f1) AS 'day',\n" +
                "    EXTRACT(quarter from f1) AS 'quarter',\n" +
                "    EXTRACT(decade from f1) AS 'decade',\n" +
                "    EXTRACT(century from f1) AS 'century',\n" +
                "    EXTRACT(millennium from f1) AS 'millennium',\n" +
                "    EXTRACT(isoyear from f1) AS 'isoyear',\n" +
                "    EXTRACT(week from f1) AS 'week',\n" +
                "    EXTRACT(dow from f1) AS 'dow',\n" +
                "    EXTRACT(isodow from f1) AS 'isodow',\n" +
                "    EXTRACT(doy from f1) AS 'doy',\n" +
                "    EXTRACT(epoch from f1) AS 'epoch'\n" +
                "    FROM DATE_TBL;\n" +
                "     date      | year  | month | day | quarter | decade | century | millennium | isoyear | week | dow | isodow | doy |     epoch     \n" +
                "---------------+-------+-------+-----+---------+--------+---------+------------+---------+------+-----+--------+-----+---------------\n" +
                " 04-09-1957    |  1957 |     4 |   9 |       2 |    195 |      20 |          2 |    1957 |   15 |   3 |      2 |  99 |    -401760000\n" +
                " 06-13-1957    |  1957 |     6 |  13 |       2 |    195 |      20 |          2 |    1957 |   24 |   5 |      4 | 164 |    -396144000\n" +
                " 02-28-1996    |  1996 |     2 |  28 |       1 |    199 |      20 |          2 |    1996 |    9 |   4 |      3 |  59 |     825465600\n" +
                " 02-29-1996    |  1996 |     2 |  29 |       1 |    199 |      20 |          2 |    1996 |    9 |   5 |      4 |  60 |     825552000\n" +
                " 03-01-1996    |  1996 |     3 |   1 |       1 |    199 |      20 |          2 |    1996 |    9 |   6 |      5 |  61 |     825638400\n" +
                " 03-02-1996    |  1996 |     3 |   2 |       1 |    199 |      20 |          2 |    1996 |    9 |   7 |      6 |  62 |     825724800\n" +
                " 02-28-1997    |  1997 |     2 |  28 |       1 |    199 |      20 |          2 |    1997 |    9 |   6 |      5 |  59 |     857088000\n" +
                " 03-01-1997    |  1997 |     3 |   1 |       1 |    199 |      20 |          2 |    1997 |    9 |   7 |      6 |  60 |     857174400\n" +
                " 03-02-1997    |  1997 |     3 |   2 |       1 |    199 |      20 |          2 |    1997 |    9 |   1 |      7 |  61 |     857260800\n" +
                " 04-01-2000    |  2000 |     4 |   1 |       2 |    200 |      20 |          2 |    2000 |   13 |   7 |      6 |  92 |     954547200\n" +
                " 04-02-2000    |  2000 |     4 |   2 |       2 |    200 |      20 |          2 |    2000 |   13 |   1 |      7 |  93 |     954633600\n" +
                " 04-03-2000    |  2000 |     4 |   3 |       2 |    200 |      20 |          2 |    2000 |   14 |   2 |      1 |  94 |     954720000\n" +
                " 04-08-2038    |  2038 |     4 |   8 |       2 |    203 |      21 |          3 |    2038 |   14 |   5 |      4 |  98 |    2154297600\n" +
                " 04-09-2039    |  2039 |     4 |   9 |       2 |    203 |      21 |          3 |    2039 |   14 |   7 |      6 |  99 |    2185920000\n" +
                " 04-10-2040    |  2040 |     4 |  10 |       2 |    204 |      21 |          3 |    2040 |   15 |   3 |      2 | 101 |    2217628800\n" +
                "               |       |       |     |         |        |         |            |         |      |     |        |     |               "
                //" 04-10-2040 BC | -2040 |     4 |  10 |       2 |   -204 |     -21 |         -3 |   -2040 |   15 |   2 |      1 | 100 | -126503251200"
        );
    }
}
