package org.dbsp.sqlCompiler.compiler;

import org.dbsp.sqlCompiler.ir.expression.DBSPExpression;
import org.dbsp.sqlCompiler.ir.expression.DBSPTupleExpression;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPBoolLiteral;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPDoubleLiteral;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPGeoPointLiteral;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPI32Literal;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPI64Literal;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPLiteral;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPStringLiteral;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPVecLiteral;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPZSetLiteral;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeBool;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeDouble;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeInteger;
import org.junit.Ignore;
import org.junit.Test;

/**
 * Runs tests using the JIT compiler backend and runtime.
 */
public class JitTests extends EndToEndTests {
    @Override
    void testQuery(String query, DBSPZSetLiteral.Contents expectedOutput) {
        DBSPZSetLiteral.Contents input = this.createInput();
        super.testQueryBase(query, false, true, true, new InputOutputPair(input, expectedOutput));
    }

    // All the @Ignore-ed tests below should eventually pass.

    @Test @Override @Ignore("Crash in JIT validation https://github.com/feldera/dbsp/issues/141")
    public void aggregateDistinctTest() {
        String query = "SELECT SUM(DISTINCT T.COL1), SUM(T.COL2) FROM T";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(
                        new DBSPI32Literal(10, true), new DBSPDoubleLiteral(13.0, true))));
    }

    @Test @Override @Ignore("Crash in JIT validation https://github.com/feldera/dbsp/issues/141")
    public void constAggregateDoubleExpression() {
        String query = "SELECT 34 / SUM (1), 20 / SUM(2) FROM T GROUP BY COL1";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(
                        new DBSPI32Literal(17, true), new DBSPI32Literal(5, true))));
    }

    @Test @Override @Ignore("Runtime memory allocation error https://github.com/feldera/dbsp/issues/145")
    public void testConcat() {
        String query = "SELECT T.COL4 || ' ' || T.COL4 FROM T";
        DBSPExpression lit = new DBSPTupleExpression(new DBSPStringLiteral("Hi Hi"));
        this.testQuery(query, new DBSPZSetLiteral.Contents(lit, lit));
    }

    @Test @Override @Ignore("Crash in JIT validation https://github.com/feldera/dbsp/issues/141")
    public void maxTest() {
        String query = "SELECT MAX(T.COL1) FROM T";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(
                        new DBSPI32Literal(10, true))));
    }

    @Test @Override @Ignore("Crash in JIT validation https://github.com/feldera/dbsp/issues/141")
    public void constAggregateExpression() {
        String query = "SELECT 34 / SUM (1) FROM T GROUP BY COL1";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(
                        new DBSPI32Literal(17, true))));
    }

    @Test @Override @Ignore("Crash in JIT validation https://github.com/feldera/dbsp/issues/141")
    public void maxConst() {
        String query = "SELECT MAX(6) FROM T";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(
                        new DBSPI32Literal(6, true))));
    }

    @Test @Override @Ignore("Runtime memory allocation error https://github.com/feldera/dbsp/issues/145")
    public void intersectTest() {
        String query = "SELECT * FROM T INTERSECT (SELECT * FROM T)";
        this.testQuery(query, this.createInput());
    }

    @Test @Override @Ignore("Runtime memory allocation error https://github.com/feldera/dbsp/issues/145")
    public void fullOuterJoinTest() {
        String query = "SELECT T1.COL3, T2.COL3 FROM T AS T1 FULL OUTER JOIN T AS T2 ON T1.COL1 = T2.COL5";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(DBSPBoolLiteral.NULLABLE_FALSE, DBSPBoolLiteral.NONE),
                new DBSPTupleExpression(DBSPBoolLiteral.NULLABLE_TRUE, DBSPBoolLiteral.NONE),
                new DBSPTupleExpression(DBSPBoolLiteral.NONE, DBSPBoolLiteral.NULLABLE_FALSE),
                new DBSPTupleExpression(DBSPBoolLiteral.NONE, DBSPBoolLiteral.NULLABLE_TRUE)
        ));
    }

    @Test @Override @Ignore("Runtime memory allocation error https://github.com/feldera/dbsp/issues/145")
    public void rightOuterJoinTest() {
        String query = "SELECT T1.COL3, T2.COL3 FROM T AS T1 RIGHT JOIN T AS T2 ON T1.COL1 = T2.COL5";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(DBSPBoolLiteral.NONE, DBSPBoolLiteral.FALSE),
                new DBSPTupleExpression(DBSPBoolLiteral.NONE, DBSPBoolLiteral.TRUE)
        ));
    }

    @Test @Override @Ignore("Runtime memory allocation error https://github.com/feldera/dbsp/issues/145")
    public void leftOuterJoinTest() {
        String query = "SELECT T1.COL3, T2.COL3 FROM T AS T1 LEFT JOIN T AS T2 ON T1.COL1 = T2.COL5";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(DBSPBoolLiteral.FALSE, DBSPBoolLiteral.NONE),
                new DBSPTupleExpression(DBSPBoolLiteral.TRUE, DBSPBoolLiteral.NONE)
        ));
    }

    @Test @Override @Ignore("SIGSEGV at runtime https://github.com/feldera/dbsp/issues/147")
    public void optionAggregateTest() {
        String query = "SELECT SUM(T.COL5) FROM T";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(
                        new DBSPI32Literal(1, true))));
    }

    @Test @Override @Ignore("NaN comparison does not work https://github.com/feldera/dbsp/issues/146")
    public void floatDivTest() {
        String query = "SELECT T.COL6 / T.COL6 FROM T";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(DBSPLiteral.none(
                        DBSPTypeDouble.NULLABLE_INSTANCE)),
                new DBSPTupleExpression(new DBSPDoubleLiteral(Double.NaN, true))));
    }

    @Test @Override @Ignore("Crash in JIT validation https://github.com/feldera/dbsp/issues/141")
    public void inTest() {
        String query = "SELECT 3 in (SELECT COL5 FROM T)";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(DBSPLiteral.none(DBSPTypeBool.NULLABLE_INSTANCE))));
    }

    @Test @Override @Ignore("Crash in JIT validation https://github.com/feldera/dbsp/issues/141")
    public void groupByCountTest() {
        String query = "SELECT COL1, COUNT(col2) FROM T GROUP BY COL1, COL3";
        DBSPExpression row =  new DBSPTupleExpression(new DBSPI32Literal(10), new DBSPI64Literal(1));
        this.testQuery(query, new DBSPZSetLiteral.Contents( row, row));
    }

    @Test @Override @Ignore("Runtime memory allocation error https://github.com/feldera/dbsp/issues/145")
    public void cartesianTest() {
        String query = "SELECT * FROM T, T AS X";
        DBSPExpression inResult = DBSPTupleExpression.flatten(e0, e0);
        DBSPZSetLiteral.Contents result = new DBSPZSetLiteral.Contents( inResult);
        result.add(DBSPTupleExpression.flatten(e0, e1));
        result.add(DBSPTupleExpression.flatten(e1, e0));
        result.add(DBSPTupleExpression.flatten(e1, e1));
        this.testQuery(query, result);
    }

    @Test @Override @Ignore("Runtime memory allocation error https://github.com/feldera/dbsp/issues/145")
    public void joinNullableTest() {
        String query = "SELECT T1.COL3, T2.COL3 FROM T AS T1 JOIN T AS T2 ON T1.COL1 = T2.COL5";
        this.testQuery(query, empty);
    }

    @Test @Override @Ignore("Runtime memory allocation error https://github.com/feldera/dbsp/issues/145")
    public void joinTest() {
        String query = "SELECT T1.COL3, T2.COL3 FROM T AS T1 JOIN T AS T2 ON T1.COL1 = T2.COL1";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(DBSPBoolLiteral.FALSE, DBSPBoolLiteral.FALSE),
                new DBSPTupleExpression(DBSPBoolLiteral.FALSE, DBSPBoolLiteral.TRUE),
                new DBSPTupleExpression(DBSPBoolLiteral.TRUE,  DBSPBoolLiteral.FALSE),
                new DBSPTupleExpression(DBSPBoolLiteral.TRUE,  DBSPBoolLiteral.TRUE)));
    }

    @Test @Override @Ignore("Crash in JIT validation https://github.com/feldera/dbsp/issues/141")
    public void groupBySumTest() {
        String query = "SELECT COL1, SUM(col2) FROM T GROUP BY COL1, COL3";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(new DBSPI32Literal(10), new DBSPDoubleLiteral(1)),
                new DBSPTupleExpression(new DBSPI32Literal(10), new DBSPDoubleLiteral(12))));
    }

    @Test @Override @Ignore("Sum operator produces wrong results https://github.com/feldera/dbsp/issues/144")
    public void aggregateFalseTest() {
        String query = "SELECT SUM(T.COL1) FROM T WHERE FALSE";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(
                        DBSPLiteral.none(DBSPTypeInteger.SIGNED_32.setMayBeNull(true)))));
    }

    @Test @Override @Ignore("Crash in JIT validation https://github.com/feldera/dbsp/issues/141")
    public void aggregateFloatTest() {
        String query = "SELECT SUM(T.COL2) FROM T";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(
                        new DBSPDoubleLiteral(13.0, true))));
    }

    @Test @Override @Ignore("Crash in JIT validation")
    public void aggregateTest() {
        String query = "SELECT SUM(T.COL1) FROM T";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(
                        new DBSPI32Literal(20, true))));
    }

    @Test @Override @Ignore("Uses Decimals, not yet supported by JIT")
    public void correlatedAggregate() {
        String query = "SELECT Sum(r.COL1 * r.COL5) FROM T r\n" +
                "WHERE\n" +
                "0.5 * (SELECT Sum(r1.COL5) FROM T r1) =\n" +
                "(SELECT Sum(r2.COL5) FROM T r2 WHERE r2.COL1 = r.COL1)";
        this.testQuery(query, new DBSPZSetLiteral.Contents(new DBSPTupleExpression(
                DBSPLiteral.none(DBSPTypeInteger.SIGNED_32.setMayBeNull(true)))));
    }

    @Test @Override @Ignore("WINDOWS not yet implemented")
    public void overTest() {
        DBSPExpression t = new DBSPTupleExpression(new DBSPI32Literal(10), new DBSPI64Literal(2));
        String query = "SELECT T.COL1, COUNT(*) OVER (ORDER BY T.COL1 RANGE UNBOUNDED PRECEDING) FROM T";
        this.testQuery(query, new DBSPZSetLiteral.Contents(t, t));
    }

    // WINDOWS not yet implemented
    @Test @Override @Ignore("WINDOWS not yet implemented")
    public void overSumTest() {
        DBSPExpression t = new DBSPTupleExpression(new DBSPI32Literal(10), new DBSPDoubleLiteral(13.0));
        String query = "SELECT T.COL1, SUM(T.COL2) OVER (ORDER BY T.COL1 RANGE UNBOUNDED PRECEDING) FROM T";
        this.testQuery(query, new DBSPZSetLiteral.Contents(t, t));
    }

    @Test @Override @Ignore("WINDOWS not yet implemented")
    public void overTwiceTest() {
        DBSPExpression t = new DBSPTupleExpression(
                new DBSPI32Literal(10),
                new DBSPDoubleLiteral(13.0),
                new DBSPI64Literal(2));
        String query = "SELECT T.COL1, " +
                "SUM(T.COL2) OVER (ORDER BY T.COL1 RANGE UNBOUNDED PRECEDING), " +
                "COUNT(*) OVER (ORDER BY T.COL1 RANGE UNBOUNDED PRECEDING) FROM T";
        this.testQuery(query, new DBSPZSetLiteral.Contents(t, t));
    }

    @Test @Override @Ignore("Average not yet implemented")
    public void averageTest() {
        String query = "SELECT AVG(T.COL1) FROM T";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(
                        new DBSPI32Literal(10, true))));
    }

    @Test @Override @Ignore("Average not yet implemented")
    public void constAggregateExpression2() {
        String query = "SELECT 34 / AVG (1) FROM T GROUP BY COL1";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(
                        new DBSPI32Literal(34, true))));
    }

    @Test @Override @Ignore("WINDOWS not yet implemented")
    public void overConstantWindowTest() {
        DBSPExpression t = new DBSPTupleExpression(
                new DBSPI32Literal(10),
                new DBSPI64Literal(2));
        String query = "SELECT T.COL1, " +
                "COUNT(*) OVER (ORDER BY T.COL1 RANGE BETWEEN 2 PRECEDING AND 1 PRECEDING) FROM T";
        this.testQuery(query, new DBSPZSetLiteral.Contents(t, t));
    }

    @Test @Override @Ignore("WINDOWS not yet implemented")
    public void overTwiceDifferentTest() {
        DBSPExpression t = new DBSPTupleExpression(
                new DBSPI32Literal(10),
                new DBSPDoubleLiteral(13.0),
                new DBSPI64Literal(2));
        String query = "SELECT T.COL1, " +
                "SUM(T.COL2) OVER (ORDER BY T.COL1 RANGE UNBOUNDED PRECEDING), " +
                "COUNT(*) OVER (ORDER BY T.COL1 RANGE BETWEEN 2 PRECEDING AND 1 PRECEDING) FROM T";
        this.testQuery(query, new DBSPZSetLiteral.Contents(t, t));
    }

    @Test @Override @Ignore("ORDER BY not yet implemented")
    public void orderbyTest() {
        String query = "SELECT * FROM T ORDER BY T.COL2";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPVecLiteral(e1, e0)
        ));
    }

    @Test @Override @Ignore("ORDER BY not yet implemented")
    public void orderbyDescendingTest() {
        String query = "SELECT * FROM T ORDER BY T.COL2 DESC";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPVecLiteral(e0, e1)
        ));
    }

    @Test @Override @Ignore("ORDER BY not yet implemented")
    public void orderby2Test() {
        String query = "SELECT * FROM T ORDER BY T.COL2, T.COL1";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPVecLiteral(e1, e0)
        ));
    }

    @Test @Override @Ignore("GEO POINT not yet implemented")
    public void geoPointTest() {
        String query = "SELECT ST_POINT(0, 0)";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(
                        new DBSPGeoPointLiteral(null,
                                new DBSPDoubleLiteral(0), new DBSPDoubleLiteral(0)).some())));
    }

    @Test @Override @Ignore("GEO POINT not yet implemented")
    public void geoDistanceTest() {
        String query = "SELECT ST_DISTANCE(ST_POINT(0, 0), ST_POINT(0,1))";
        this.testQuery(query, new DBSPZSetLiteral.Contents(
                new DBSPTupleExpression(new DBSPDoubleLiteral(1).some())));
    }
}