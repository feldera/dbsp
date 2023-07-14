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

package org.dbsp.sqlCompiler.compiler.postgres;

import org.dbsp.sqlCompiler.compiler.backend.DBSPCompiler;
import org.junit.Ignore;
import org.junit.Test;

/**
 * Tests manually adapted from
 * https://github.com/postgres/postgres/blob/master/src/test/regress/expected/date.out
 */
@SuppressWarnings("JavadocLinkAsPlainText")
public class PostgresDateTests extends PostgresBaseTest {
    @Override
    public void prepareData(DBSPCompiler compiler) {
        String data = "CREATE TABLE DATE_TBL (f1 date);\n" +
                "INSERT INTO DATE_TBL VALUES ('1957-04-09');\n" +
                "INSERT INTO DATE_TBL VALUES ('1957-06-13');\n" +
                "INSERT INTO DATE_TBL VALUES ('1996-02-28');\n" +
                "INSERT INTO DATE_TBL VALUES ('1996-02-29');\n" +
                "INSERT INTO DATE_TBL VALUES ('1996-03-01');\n" +
                "INSERT INTO DATE_TBL VALUES ('1996-03-02');\n" +
                "INSERT INTO DATE_TBL VALUES ('1997-02-28');\n" +
                // illegal date; this fails in Postgres, but inserts a NULL in Calcite.
                "INSERT INTO DATE_TBL VALUES ('1997-02-29');\n" +
                "INSERT INTO DATE_TBL VALUES ('1997-03-01');\n" +
                "INSERT INTO DATE_TBL VALUES ('1997-03-02');\n" +
                "INSERT INTO DATE_TBL VALUES ('2000-04-01');\n" +
                "INSERT INTO DATE_TBL VALUES ('2000-04-02');\n" +
                "INSERT INTO DATE_TBL VALUES ('2000-04-03');\n" +
                "INSERT INTO DATE_TBL VALUES ('2038-04-08');\n" +
                "INSERT INTO DATE_TBL VALUES ('2039-04-09');\n" +
                "INSERT INTO DATE_TBL VALUES ('2040-04-10');\n";
        // Calcite does not seem to support dates BC
        //"INSERT INTO DATE_TBL VALUES ('2040-04-10 BC');";
        compiler.compileStatements(data);
    }

    @Test
    public void testSelect() {
        this.q("SELECT f1 FROM DATE_TBL;\n" +
                "f1\n" +
                "---------------\n" +
                "04-09-1957\n" +
                "06-13-1957\n" +
                "02-28-1996\n" +
                "02-29-1996\n" +
                "03-01-1996\n" +
                "03-02-1996\n" +
                "02-28-1997\n" +
                "03-01-1997\n" +
                "03-02-1997\n" +
                "04-01-2000\n" +
                "04-02-2000\n" +
                "04-03-2000\n" +
                "04-08-2038\n" +
                "04-09-2039\n" +
                "04-10-2040\n" +
                "null");  // The invalid date
    }

    @Test
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

    @Test
    public void testBetween() {
        this.q("SELECT f1 FROM DATE_TBL\n" +
                "  WHERE f1 BETWEEN '2000-01-01' AND '2001-01-01';\n" +
                " f1\n" +
                "----------------\n" +
                "04-01-2000\n" +
                "04-02-2000\n" +
                "04-03-2000");
    }

    // SELECT date '1999-01-08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '1999-01-18';
    //    date
    //------------
    // 1999-01-18
    //(1 row)
    //
    //SELECT date '1/8/1999';
    //ERROR:  date/time field value out of range: "1/8/1999"
    //LINE 1: SELECT date '1/8/1999';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '1/18/1999';
    //ERROR:  date/time field value out of range: "1/18/1999"
    //LINE 1: SELECT date '1/18/1999';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '18/1/1999';
    //ERROR:  date/time field value out of range: "18/1/1999"
    //LINE 1: SELECT date '18/1/1999';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '01/02/03';
    //    date
    //------------
    // 2001-02-03
    //(1 row)
    //
    //SELECT date '19990108';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '990108';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '1999.008';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date 'J2451187';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date 'January 8, 99 BC';
    //ERROR:  date/time field value out of range: "January 8, 99 BC"
    //LINE 1: SELECT date 'January 8, 99 BC';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '99-Jan-08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '1999-Jan-08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '08-Jan-99';
    //ERROR:  date/time field value out of range: "08-Jan-99"
    //LINE 1: SELECT date '08-Jan-99';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '08-Jan-1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date 'Jan-08-99';
    //ERROR:  date/time field value out of range: "Jan-08-99"
    //LINE 1: SELECT date 'Jan-08-99';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date 'Jan-08-1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '99-08-Jan';
    //ERROR:  invalid input syntax for type date: "99-08-Jan"
    //LINE 1: SELECT date '99-08-Jan';
    //                    ^
    //SELECT date '1999-08-Jan';
    //ERROR:  invalid input syntax for type date: "1999-08-Jan"
    //LINE 1: SELECT date '1999-08-Jan';
    //                    ^
    //SELECT date '99 Jan 08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '1999 Jan 08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '08 Jan 99';
    //ERROR:  date/time field value out of range: "08 Jan 99"
    //LINE 1: SELECT date '08 Jan 99';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '08 Jan 1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date 'Jan 08 99';
    //ERROR:  date/time field value out of range: "Jan 08 99"
    //LINE 1: SELECT date 'Jan 08 99';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date 'Jan 08 1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '99 08 Jan';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '1999 08 Jan';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '99-01-08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '1999-01-08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '08-01-99';
    //ERROR:  date/time field value out of range: "08-01-99"
    //LINE 1: SELECT date '08-01-99';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '08-01-1999';
    //ERROR:  date/time field value out of range: "08-01-1999"
    //LINE 1: SELECT date '08-01-1999';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '01-08-99';
    //ERROR:  date/time field value out of range: "01-08-99"
    //LINE 1: SELECT date '01-08-99';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '01-08-1999';
    //ERROR:  date/time field value out of range: "01-08-1999"
    //LINE 1: SELECT date '01-08-1999';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '99-08-01';
    //    date
    //------------
    // 1999-08-01
    //(1 row)
    //
    //SELECT date '1999-08-01';
    //    date
    //------------
    // 1999-08-01
    //(1 row)
    //
    //SELECT date '99 01 08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '1999 01 08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '08 01 99';
    //ERROR:  date/time field value out of range: "08 01 99"
    //LINE 1: SELECT date '08 01 99';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '08 01 1999';
    //ERROR:  date/time field value out of range: "08 01 1999"
    //LINE 1: SELECT date '08 01 1999';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '01 08 99';
    //ERROR:  date/time field value out of range: "01 08 99"
    //LINE 1: SELECT date '01 08 99';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '01 08 1999';
    //ERROR:  date/time field value out of range: "01 08 1999"
    //LINE 1: SELECT date '01 08 1999';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '99 08 01';
    //    date
    //------------
    // 1999-08-01
    //(1 row)
    //
    //SELECT date '1999 08 01';
    //    date
    //------------
    // 1999-08-01
    //(1 row)
    //
    //SET datestyle TO dmy;
    //SELECT date 'January 8, 1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '1999-01-08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '1999-01-18';
    //    date
    //------------
    // 1999-01-18
    //(1 row)
    //
    //SELECT date '1/8/1999';
    //    date
    //------------
    // 1999-08-01
    //(1 row)
    //
    //SELECT date '1/18/1999';
    //ERROR:  date/time field value out of range: "1/18/1999"
    //LINE 1: SELECT date '1/18/1999';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '18/1/1999';
    //    date
    //------------
    // 1999-01-18
    //(1 row)
    //
    //SELECT date '01/02/03';
    //    date
    //------------
    // 2003-02-01
    //(1 row)
    //
    //SELECT date '19990108';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '990108';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '1999.008';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date 'J2451187';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date 'January 8, 99 BC';
    //     date
    //---------------
    // 0099-01-08 BC
    //(1 row)
    //
    //SELECT date '99-Jan-08';
    //ERROR:  date/time field value out of range: "99-Jan-08"
    //LINE 1: SELECT date '99-Jan-08';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '1999-Jan-08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '08-Jan-99';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '08-Jan-1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date 'Jan-08-99';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date 'Jan-08-1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '99-08-Jan';
    //ERROR:  invalid input syntax for type date: "99-08-Jan"
    //LINE 1: SELECT date '99-08-Jan';
    //                    ^
    //SELECT date '1999-08-Jan';
    //ERROR:  invalid input syntax for type date: "1999-08-Jan"
    //LINE 1: SELECT date '1999-08-Jan';
    //                    ^
    //SELECT date '99 Jan 08';
    //ERROR:  date/time field value out of range: "99 Jan 08"
    //LINE 1: SELECT date '99 Jan 08';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '1999 Jan 08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '08 Jan 99';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '08 Jan 1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date 'Jan 08 99';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date 'Jan 08 1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '99 08 Jan';
    //ERROR:  invalid input syntax for type date: "99 08 Jan"
    //LINE 1: SELECT date '99 08 Jan';
    //                    ^
    //SELECT date '1999 08 Jan';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '99-01-08';
    //ERROR:  date/time field value out of range: "99-01-08"
    //LINE 1: SELECT date '99-01-08';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '1999-01-08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '08-01-99';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '08-01-1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '01-08-99';
    //    date
    //------------
    // 1999-08-01
    //(1 row)
    //
    //SELECT date '01-08-1999';
    //    date
    //------------
    // 1999-08-01
    //(1 row)
    //
    //SELECT date '99-08-01';
    //ERROR:  date/time field value out of range: "99-08-01"
    //LINE 1: SELECT date '99-08-01';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '1999-08-01';
    //    date
    //------------
    // 1999-08-01
    //(1 row)
    //
    //SELECT date '99 01 08';
    //ERROR:  date/time field value out of range: "99 01 08"
    //LINE 1: SELECT date '99 01 08';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '1999 01 08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '08 01 99';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '08 01 1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '01 08 99';
    //    date
    //------------
    // 1999-08-01
    //(1 row)
    //
    //SELECT date '01 08 1999';
    //    date
    //------------
    // 1999-08-01
    //(1 row)
    //
    //SELECT date '99 08 01';
    //ERROR:  date/time field value out of range: "99 08 01"
    //LINE 1: SELECT date '99 08 01';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '1999 08 01';
    //    date
    //------------
    // 1999-08-01
    //(1 row)
    //
    //SET datestyle TO mdy;
    //SELECT date 'January 8, 1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '1999-01-08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '1999-01-18';
    //    date
    //------------
    // 1999-01-18
    //(1 row)
    //
    //SELECT date '1/8/1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '1/18/1999';
    //    date
    //------------
    // 1999-01-18
    //(1 row)
    //
    //SELECT date '18/1/1999';
    //ERROR:  date/time field value out of range: "18/1/1999"
    //LINE 1: SELECT date '18/1/1999';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '01/02/03';
    //    date
    //------------
    // 2003-01-02
    //(1 row)
    //
    //SELECT date '19990108';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '990108';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '1999.008';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date 'J2451187';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date 'January 8, 99 BC';
    //     date
    //---------------
    // 0099-01-08 BC
    //(1 row)
    //
    //SELECT date '99-Jan-08';
    //ERROR:  date/time field value out of range: "99-Jan-08"
    //LINE 1: SELECT date '99-Jan-08';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '1999-Jan-08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '08-Jan-99';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '08-Jan-1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date 'Jan-08-99';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date 'Jan-08-1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '99-08-Jan';
    //ERROR:  invalid input syntax for type date: "99-08-Jan"
    //LINE 1: SELECT date '99-08-Jan';
    //                    ^
    //SELECT date '1999-08-Jan';
    //ERROR:  invalid input syntax for type date: "1999-08-Jan"
    //LINE 1: SELECT date '1999-08-Jan';
    //                    ^
    //SELECT date '99 Jan 08';
    //ERROR:  invalid input syntax for type date: "99 Jan 08"
    //LINE 1: SELECT date '99 Jan 08';
    //                    ^
    //SELECT date '1999 Jan 08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '08 Jan 99';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '08 Jan 1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date 'Jan 08 99';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date 'Jan 08 1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '99 08 Jan';
    //ERROR:  invalid input syntax for type date: "99 08 Jan"
    //LINE 1: SELECT date '99 08 Jan';
    //                    ^
    //SELECT date '1999 08 Jan';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '99-01-08';
    //ERROR:  date/time field value out of range: "99-01-08"
    //LINE 1: SELECT date '99-01-08';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '1999-01-08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '08-01-99';
    //    date
    //------------
    // 1999-08-01
    //(1 row)
    //
    //SELECT date '08-01-1999';
    //    date
    //------------
    // 1999-08-01
    //(1 row)
    //
    //SELECT date '01-08-99';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '01-08-1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '99-08-01';
    //ERROR:  date/time field value out of range: "99-08-01"
    //LINE 1: SELECT date '99-08-01';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '1999-08-01';
    //    date
    //------------
    // 1999-08-01
    //(1 row)
    //
    //SELECT date '99 01 08';
    //ERROR:  date/time field value out of range: "99 01 08"
    //LINE 1: SELECT date '99 01 08';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '1999 01 08';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '08 01 99';
    //    date
    //------------
    // 1999-08-01
    //(1 row)
    //
    //SELECT date '08 01 1999';
    //    date
    //------------
    // 1999-08-01
    //(1 row)
    //
    //SELECT date '01 08 99';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '01 08 1999';
    //    date
    //------------
    // 1999-01-08
    //(1 row)
    //
    //SELECT date '99 08 01';
    //ERROR:  date/time field value out of range: "99 08 01"
    //LINE 1: SELECT date '99 08 01';
    //                    ^
    //HINT:  Perhaps you need a different "datestyle" setting.
    //SELECT date '1999 08 01';
    //    date
    //------------
    // 1999-08-01
    //(1 row)
    //
    //-- Check upper and lower limits of date range
    //SELECT date '4714-11-24 BC';
    //     date
    //---------------
    // 4714-11-24 BC
    //(1 row)
    //
    //SELECT date '4714-11-23 BC';  -- out of range
    //ERROR:  date out of range: "4714-11-23 BC"
    //LINE 1: SELECT date '4714-11-23 BC';
    //                    ^
    //SELECT date '5874897-12-31';
    //     date
    //---------------
    // 5874897-12-31
    //(1 row)
    //
    //SELECT date '5874898-01-01';  -- out of range
    //ERROR:  date out of range: "5874898-01-01"
    //LINE 1: SELECT date '5874898-01-01';
    //                    ^
    //-- Test non-error-throwing API
    //SELECT pg_input_is_valid('now', 'date');
    // pg_input_is_valid
    //-------------------
    // t
    //(1 row)
    //
    //SELECT pg_input_is_valid('garbage', 'date');
    // pg_input_is_valid
    //-------------------
    // f
    //(1 row)
    //
    //SELECT pg_input_is_valid('6874898-01-01', 'date');
    // pg_input_is_valid
    //-------------------
    // f
    //(1 row)
    //
    //SELECT pg_input_error_message('garbage', 'date');
    //            pg_input_error_message
    //-----------------------------------------------
    // invalid input syntax for type date: "garbage"
    //(1 row)
    //
    //SELECT pg_input_error_message('6874898-01-01', 'date');
    //       pg_input_error_message
    //------------------------------------
    // date out of range: "6874898-01-01"
    //(1 row)

    @Test
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

    // There is no 'epoch' date in Calcite
    // SELECT f1 - date 'epoch' AS "Days From Epoch" FROM DATE_TBL;
    // SELECT date 'yesterday' - date 'today' AS "One day";
    // One day
    //---------
    //      -1
    //(1 row)
    //
    //SELECT date 'today' - date 'tomorrow' AS "One day";
    // One day
    //---------
    //      -1
    //(1 row)
    //
    //SELECT date 'yesterday' - date 'tomorrow' AS "Two days";
    // Two days
    //----------
    //       -2
    //(1 row)
    //
    //SELECT date 'tomorrow' - date 'today' AS "One day";
    // One day
    //---------
    //       1
    //(1 row)
    //
    //SELECT date 'today' - date 'yesterday' AS "One day";
    // One day
    //---------
    //       1
    //(1 row)
    //
    //SELECT date 'tomorrow' - date 'yesterday' AS "Two days";
    // Two days
    //----------
    //        2
    //(1 row)

    @Test
    public void testExtract() {
        // SELECT f1 as "date",
        //    date_part('year', f1) AS year,
        //    date_part('month', f1) AS month,
        //    date_part('day', f1) AS day,
        //    date_part('quarter', f1) AS quarter,
        //    date_part('decade', f1) AS decade,
        //    date_part('century', f1) AS century,
        //    date_part('millennium', f1) AS millennium,
        //    date_part('isoyear', f1) AS isoyear,
        //    date_part('week', f1) AS week,
        //    date_part('dow', f1) AS dow,
        //    date_part('isodow', f1) AS isodow,
        //    date_part('doy', f1) AS doy,
        //    date_part('julian', f1) AS julian,
        //    date_part('epoch', f1) AS epoch
        //    FROM date_tbl


        // Deleted 'Julian' column
        // Incremented DOW column
        // Added null row
        this.q("SELECT f1 as \"date\",\n" +
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

    @Test
    public void testEpoch() {
        this.q("SELECT EXTRACT(EPOCH FROM DATE '1970-01-01');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    @Test
    public void testCentury() {
        this.q("SELECT EXTRACT(CENTURY FROM DATE '0001-01-01');\n" +
                " extract \n" +
                "---------\n" +
                "       1");
    }

    @Test
    public void testCentury1() {
        this.q("SELECT EXTRACT(CENTURY FROM DATE '1900-12-31');\n" +
                " extract \n" +
                "---------\n" +
                "      19");
    }

    @Test
    public void testCentury2() {
        this.q("SELECT EXTRACT(CENTURY FROM DATE '1901-01-01');\n" +
                " extract \n" +
                "---------\n" +
                "      20");
    }

    @Test
    public void testCentury3() {
        this.q("SELECT EXTRACT(CENTURY FROM DATE '2000-12-31');\n" +
                " extract \n" +
                "---------\n" +
                "      20");
    }

    @Test
    public void testCentury4() {
        this.q("SELECT EXTRACT(CENTURY FROM DATE '2001-01-01');\n" +
                " extract \n" +
                "---------\n" +
                "      21");
    }

    @Test
    public void testMillennium() {
        this.q("SELECT EXTRACT(MILLENNIUM FROM DATE '0001-01-01');\n" +
                " extract \n" +
                "---------\n" +
                "       1");
    }

    @Test
    public void testMillennium1() {
        this.q("SELECT EXTRACT(MILLENNIUM FROM DATE '1000-12-31');\n" +
                " extract \n" +
                "---------\n" +
                "       1");
    }

    @Test
    public void testMillennium2() {
        this.q("SELECT EXTRACT(MILLENNIUM FROM DATE '2000-12-31');\n" +
                " extract \n" +
                "---------\n" +
                "       2");
    }

    // CURRENT_DATE not supported
    // SELECT EXTRACT(CENTURY FROM CURRENT_DATE)>=21 AS True;     -- true

    @Test
    public void testMillennium3() {
        this.q("SELECT EXTRACT(MILLENNIUM FROM DATE '2001-01-01');\n" +
                " extract \n" +
                "---------\n" +
                "       3");
    }

    @Test
    public void testDecade() {
        this.q("SELECT EXTRACT(DECADE FROM DATE '1994-12-25');\n" +
                " extract \n" +
                "---------\n" +
                "     199");
    }

    @Test
    public void testDecade1() {
        this.q("SELECT EXTRACT(DECADE FROM DATE '0010-01-01');\n" +
                " extract \n" +
                "---------\n" +
                "       1");
    }

    @Test
    public void testDecade2() {
        this.q("SELECT EXTRACT(DECADE FROM DATE '0009-12-31');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    @Test
    public void testMicroseconds() {
        this.q("SELECT EXTRACT(MICROSECOND FROM DATE '2020-08-11');\n" +
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

    @Test
    public void testMilliseconds() {
        this.q("SELECT EXTRACT(MILLISECOND FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    @Test
    public void testSeconds() {
        this.q("SELECT EXTRACT(SECOND FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    @Test
    public void testSeconds0() {
        this.q("SELECT SECOND(DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    @Test
    public void testMinutes() {
        this.q("SELECT EXTRACT(MINUTE FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    @Test
    public void testMinutes1() {
        this.q("SELECT MINUTE(DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    @Test
    public void testHour() {
        this.q("SELECT EXTRACT(HOUR FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    @Test
    public void testHour1() {
        this.q("SELECT HOUR(DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       0");
    }

    @Test
    public void testDay() {
        this.q("SELECT EXTRACT(DAY FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       11");
    }

    @Test
    public void testDay1() {
        this.q("SELECT DAYOFMONTH(DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       11");
    }

    @Test
    public void testMonth() {
        this.q("SELECT EXTRACT(MONTH FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       8");
    }

    @Test
    public void testMonth1() {
        this.q("SELECT MONTH(DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "       8");
    }

    @Test
    public void testYear() {
        this.q("SELECT EXTRACT(YEAR FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     2020");
    }

    @Test
    public void testYear1() {
        this.q("SELECT YEAR(DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     2020");
    }

    @Test
    public void testDecade5() {
        this.q("SELECT EXTRACT(DECADE FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     202");
    }

    @Test
    public void testCentury5() {
        this.q("SELECT EXTRACT(CENTURY FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     21");
    }

    @Test
    public void testMillennium5() {
        this.q("SELECT EXTRACT(MILLENNIUM FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     3");
    }

    @Test
    public void testIsoYear() {
        this.q("SELECT EXTRACT(ISOYEAR FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     2020");
    }

    @Test
    public void testQuarter() {
        this.q("SELECT EXTRACT(QUARTER FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     3");
    }

    @Test
    public void testWeek() {
        this.q("SELECT EXTRACT(WEEK FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     33");
    }

    @Test
    public void testDow() {
        this.q("SELECT EXTRACT(DOW FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     3");
    }

    @Test
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
    @Test
    public void testDow1() {
        // Sunday
        this.q("SELECT EXTRACT(DOW FROM DATE '2020-08-16');\n" +
                " extract \n" +
                "---------\n" +
                "     1");
    }

    @Test
    public void testIsoDow() {
        this.q("SELECT EXTRACT(ISODOW FROM DATE '2020-08-16');\n" +
                " extract \n" +
                "---------\n" +
                "     7");
    }

    @Test
    public void testDoy() {
        this.q("SELECT EXTRACT(DOY FROM DATE '2020-08-11');\n" +
                " extract \n" +
                "---------\n" +
                "     224");
    }

    // SELECT EXTRACT(TIMEZONE      FROM DATE '2020-08-11');
    //ERROR:  unit "timezone" not supported for type date
    //SELECT EXTRACT(TIMEZONE_M    FROM DATE '2020-08-11');
    //ERROR:  unit "timezone_m" not supported for type date
    //SELECT EXTRACT(TIMEZONE_H    FROM DATE '2020-08-11');
    //ERROR:  unit "timezone_h" not supported for type date

    @Test @Ignore("There are two bugs in Calcite; " +
            "now waiting for https://issues.apache.org/jira/projects/CALCITE/issues/CALCITE-5760")
    public void testDateTrunc() {
        // In the BigQuery library there is a DATE_TRUNC, but arguments are swapped
        this.q("SELECT DATE_TRUNC(DATE '1970-03-20', MILLENNIUM);\n" +
                "date_trunc        \n" +
                "--------------------------\n" +
                " Thu Jan 01 00:00:00 1001");
    }

    // SELECT DATE_TRUNC('MILLENNIUM', DATE '1970-03-20'); -- 1001-01-01
    //          date_trunc
    //------------------------------
    // Thu Jan 01 00:00:00 1001 PST
    //(1 row)
    //
    //SELECT DATE_TRUNC('CENTURY', TIMESTAMP '1970-03-20 04:30:00.00000'); -- 1901
    //        date_trunc
    //--------------------------
    // Tue Jan 01 00:00:00 1901
    //(1 row)
    //
    //SELECT DATE_TRUNC('CENTURY', DATE '1970-03-20'); -- 1901
    //          date_trunc
    //------------------------------
    // Tue Jan 01 00:00:00 1901 PST
    //(1 row)
    //
    //SELECT DATE_TRUNC('CENTURY', DATE '2004-08-10'); -- 2001-01-01
    //          date_trunc
    //------------------------------
    // Mon Jan 01 00:00:00 2001 PST
    //(1 row)
    //
    //SELECT DATE_TRUNC('CENTURY', DATE '0002-02-04'); -- 0001-01-01
    //          date_trunc
    //------------------------------
    // Mon Jan 01 00:00:00 0001 PST
    //(1 row)
    //
    //SELECT DATE_TRUNC('CENTURY', DATE '0055-08-10 BC'); -- 0100-01-01 BC
    //           date_trunc
    //---------------------------------
    // Tue Jan 01 00:00:00 0100 PST BC
    //(1 row)
    //
    //SELECT DATE_TRUNC('DECADE', DATE '1993-12-25'); -- 1990-01-01
    //          date_trunc
    //------------------------------
    // Mon Jan 01 00:00:00 1990 PST
    //(1 row)
    //
    //SELECT DATE_TRUNC('DECADE', DATE '0004-12-25'); -- 0001-01-01 BC
    //           date_trunc
    //---------------------------------
    // Sat Jan 01 00:00:00 0001 PST BC
    //(1 row)
    //
    //SELECT DATE_TRUNC('DECADE', DATE '0002-12-31 BC'); -- 0011-01-01 BC
    //           date_trunc
    //---------------------------------
    // Mon Jan 01 00:00:00 0011 PST BC

    // select 'infinity'::date > 'today'::date as t;
    // t
    //---
    // t
    //(1 row)
    //
    //select '-infinity'::date < 'today'::date as t;
    // t
    //---
    // t
    //(1 row)
    //
    //select isfinite('infinity'::date), isfinite('-infinity'::date), isfinite('today'::date);
    // isfinite | isfinite | isfinite
    //----------+----------+----------
    // f        | f        | t
    //(1 row)
    //
    //select 'infinity'::date = '+infinity'::date as t;
    // t
    //---
    // t

    @Test(expected = RuntimeException.class)
    // Calcite does not seem to support infinity dates
    public void testInfinity() {
        this.q("SELECT DATE 'infinity';\n");
    }

    // Calcite does not support make_date
    // select make_date(0, 7, 15);
    //ERROR:  date field value out of range: 0-07-15
    //select make_date(2013, 2, 30);
    //ERROR:  date field value out of range: 2013-02-30
    //select make_date(2013, 13, 1);
    //ERROR:  date field value out of range: 2013-13-01
    //select make_date(2013, 11, -1);
    //ERROR:  date field value out of range: 2013-11--1
    //select make_time(10, 55, 100.1);
    //ERROR:  time field value out of range: 10:55:100.1
    //select make_time(24, 0, 2.1);
    //ERROR:  time field value out of range: 24:00:2.1
}
