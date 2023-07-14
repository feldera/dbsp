package org.dbsp.sqlCompiler.compiler.postgres;

import org.apache.calcite.config.Lex;
import org.dbsp.sqlCompiler.circuit.DBSPCircuit;
import org.dbsp.sqlCompiler.compiler.BaseSQLTests;
import org.dbsp.sqlCompiler.compiler.CompilerOptions;
import org.dbsp.sqlCompiler.compiler.InputOutputPair;
import org.dbsp.sqlCompiler.compiler.backend.DBSPCompiler;
import org.dbsp.sqlCompiler.compiler.errors.UnimplementedException;
import org.dbsp.sqlCompiler.compiler.frontend.CalciteObject;
import org.dbsp.sqlCompiler.ir.expression.DBSPExpression;
import org.dbsp.sqlCompiler.ir.expression.DBSPTupleExpression;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPBoolLiteral;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPDateLiteral;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPDecimalLiteral;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPDoubleLiteral;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPFloatLiteral;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPI16Literal;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPI32Literal;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPI64Literal;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPIntervalMillisLiteral;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPLiteral;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPStringLiteral;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPTimestampLiteral;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPZSetLiteral;
import org.dbsp.sqlCompiler.ir.type.DBSPType;
import org.dbsp.sqlCompiler.ir.type.DBSPTypeTuple;
import org.dbsp.sqlCompiler.ir.type.DBSPTypeZSet;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeBaseType;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeBool;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeDate;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeDecimal;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeDouble;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeFloat;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeInteger;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeMillisInterval;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeString;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeTimestamp;

import javax.annotation.Nullable;
import java.math.BigDecimal;
import java.nio.charset.StandardCharsets;
import java.text.ParseException;
import java.text.SimpleDateFormat;
import java.util.Date;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

public abstract class PostgresBaseTest extends BaseSQLTests {
    /**
     * Override this method to prepare the tables on
     * which the tests are built.
     */
    public void prepareData(DBSPCompiler compiler) {}

    public CompilerOptions getOptions(boolean optimize) {
        CompilerOptions options = new CompilerOptions();
        options.ioOptions.lexicalRules = Lex.ORACLE;
        options.optimizerOptions.throwOnError = true;
        options.optimizerOptions.optimizationLevel = optimize ? 2 : 0;
        options.optimizerOptions.generateInputForEveryTable = true;
        options.optimizerOptions.incrementalize = false;
        return options;
    }

    public DBSPCompiler testCompiler(boolean optimize) {
        CompilerOptions options = this.getOptions(optimize);
        return new DBSPCompiler(options);
    }

    // Calcite is not very flexible regarding timestamp formats
    public DBSPCompiler compileQuery(String query, boolean optimize) {
        DBSPCompiler compiler = this.testCompiler(optimize);
        this.prepareData(compiler);
        compiler.compileStatement(query);
        return compiler;
    }

    static final SimpleDateFormat[] TIMESTAMP_INPUT_FORMAT = {
            new SimpleDateFormat("EEE MMM d HH:mm:ss.SSS yyyy"),
            new SimpleDateFormat("EEE MMM d HH:mm:ss yyyy G"),
            new SimpleDateFormat("EEE MMM d HH:mm:ss yyyy")
    };
    static final SimpleDateFormat TIMESTAMP_OUTPUT_FORMAT = new SimpleDateFormat("yyyy-MM-dd HH:mm:ss.S");

    /**
     * Convert a timestamp from a format like Sat Feb 16 17:32:01 1996 to
     * a format like 1996-02-16 17:32:01
     */
    public static DBSPExpression convertTimestamp(@Nullable String timestamp, boolean mayBeNull) {
        if (timestamp == null)
            return DBSPLiteral.none(DBSPTypeTimestamp.NULLABLE_INSTANCE);
        for (SimpleDateFormat input: TIMESTAMP_INPUT_FORMAT) {
            String out;
            try {
                // Calcite problems: does not support negative years, or fractional seconds ending in 0
                Date zero = new SimpleDateFormat("yyyy-MM-dd").parse("0000-01-01");
                Date converted = input.parse(timestamp);
                out = TIMESTAMP_OUTPUT_FORMAT.format(converted);
                if (converted.before(zero))
                    out = "-" + out;
            } catch (ParseException ignored) {
                continue;
            }
            return new DBSPTimestampLiteral(out, mayBeNull);
        }
        throw new RuntimeException("Could not parse " + timestamp);
    }

    static final SimpleDateFormat DATE_INPUT_FORMAT = new SimpleDateFormat("MM-dd-yyyy");
    static final SimpleDateFormat DATE_OUTPUT_FORMAT = new SimpleDateFormat("yyyy-MM-dd");

    /**
     * Convert a date from the MM-DD-YYYY format (which is used in the Postgres output)
     * to a DBSPLiteral.
     */
    static DBSPExpression convertDate(@Nullable String date) {
        if (date == null || date.isEmpty() || date.equalsIgnoreCase("null"))
            return DBSPLiteral.none(DBSPTypeDate.NULLABLE_INSTANCE);
        try {
            Date converted = DATE_INPUT_FORMAT.parse(date);
            String out = DATE_OUTPUT_FORMAT.format(converted);
            return new DBSPDateLiteral(out, true);
        } catch (ParseException ex) {
            throw new RuntimeException("Could not parse " + date);
        }
    }

    public DBSPZSetLiteral.Contents parseTable(String table, DBSPType outputType) {
        DBSPTypeZSet zset = outputType.to(DBSPTypeZSet.class);
        DBSPZSetLiteral.Contents result = DBSPZSetLiteral.Contents.emptyWithElementType(zset.elementType);
        DBSPTypeTuple tuple = zset.elementType.to(DBSPTypeTuple.class);

        String[] lines = table.split("\n", -1);
        boolean inHeader = true;
        for (String line: lines) {
            if (line.startsWith("---")) {
                inHeader = false;
                continue;
            }
            if (inHeader)
                continue;
            int comment = line.indexOf("--");
            if (comment >= 0)
                line = line.substring(0, comment);
            String[] columns = line.split("[|]");
            if (columns.length != tuple.size())
                throw new RuntimeException("Row has " + columns.length + " columns, but expected " + tuple.size());
            DBSPExpression[] values = new DBSPExpression[columns.length];
            for (int i = 0; i < columns.length; i++) {
                String column = columns[i].trim();
                DBSPType fieldType = tuple.getFieldType(i);
                if (!fieldType.is(DBSPTypeString.class) &&
                        (column.isEmpty() ||
                         column.equalsIgnoreCase("null"))) {
                    if (!fieldType.mayBeNull)
                        throw new RuntimeException("Null value in non-nullable column " + fieldType);
                    values[i] = fieldType.to(DBSPTypeBaseType.class).nullValue();
                    continue;
                }
                DBSPExpression columnValue;
                if (fieldType.is(DBSPTypeDouble.class)) {
                    double value = Double.parseDouble(column);
                    columnValue = new DBSPDoubleLiteral(value, fieldType.mayBeNull);
                } else if (fieldType.is(DBSPTypeFloat.class)) {
                    float value = Float.parseFloat(column);
                    columnValue = new DBSPFloatLiteral(value, fieldType.mayBeNull);
                } else if (fieldType.is(DBSPTypeDecimal.class)) {
                    BigDecimal value = new BigDecimal(column);
                    columnValue = new DBSPDecimalLiteral(fieldType, value);
                } else if (fieldType.is(DBSPTypeTimestamp.class)) {
                    columnValue = convertTimestamp(column, fieldType.mayBeNull);
                } else if (fieldType.is(DBSPTypeDate.class)) {
                    columnValue = convertDate(column);
                } else if (fieldType.is(DBSPTypeInteger.class)) {
                    DBSPTypeInteger intType = fieldType.to(DBSPTypeInteger.class);
                    switch (intType.getWidth()) {
                        case 16:
                            columnValue = new DBSPI16Literal(Short.parseShort(column), fieldType.mayBeNull);
                            break;
                        case 32:
                            columnValue = new DBSPI32Literal(Integer.parseInt(column), fieldType.mayBeNull);
                            break;
                        case 64:
                            columnValue = new DBSPI64Literal(Long.parseLong(column), fieldType.mayBeNull);
                            break;
                        default:
                            throw new UnimplementedException(intType);
                    }
                } else if (fieldType.is(DBSPTypeMillisInterval.class)) {
                    long value = Long.parseLong(column);
                    columnValue = new DBSPIntervalMillisLiteral(value * 86400000, fieldType.mayBeNull);
                } else if (fieldType.is(DBSPTypeString.class)) {
                    // No trim
                    columnValue = new DBSPStringLiteral(CalciteObject.EMPTY, fieldType, columns[i], StandardCharsets.UTF_8);
                } else if (fieldType.is(DBSPTypeBool.class)) {
                    boolean value = column.equalsIgnoreCase("t") || column.equalsIgnoreCase("true");
                    columnValue = new DBSPBoolLiteral(CalciteObject.EMPTY, fieldType, value);
                } else {
                    throw new UnimplementedException(fieldType);
                }
                values[i] = columnValue;
            }
            result.add(new DBSPTupleExpression(values));
        }
        if (inHeader)
            throw new RuntimeException("Could not find end of header for table");
        return result;
    }

    DBSPZSetLiteral.Contents[] getPreparedInputs(DBSPCompiler compiler) {
        DBSPZSetLiteral.Contents[] inputs = new DBSPZSetLiteral.Contents[
                compiler.getTableContents().tablesCreated.size()];
        int index = 0;
        for (String table: compiler.getTableContents().tablesCreated) {
            DBSPZSetLiteral.Contents data = compiler.getTableContents().getTableContents(table.toUpperCase());
            inputs[index++] = data;
        }
        return inputs;
    }

    void compare(String query, String expected, boolean optimize) {
        DBSPCompiler compiler = this.testCompiler(optimize);
        this.prepareData(compiler);
        compiler.compileStatement("CREATE VIEW VV AS " + query);
        compiler.optimize();
        DBSPCircuit circuit = getCircuit(compiler);
        DBSPType outputType = circuit.getOutputType(0);
        DBSPZSetLiteral.Contents result = this.parseTable(expected, outputType);
        InputOutputPair streams = new InputOutputPair(
                this.getPreparedInputs(compiler),
                new DBSPZSetLiteral.Contents[] { result }
        );
        this.addRustTestCase(query, compiler, circuit, streams);
    }

    /**
     * Test a query followed by the expected output.
     * The query ends at the semicolon.
     * Runs two test cases, one with optimizations and one without.
     * This makes sure that constant queries still exercise the runtime.
     */
    public void q(String queryAndOutput) {
        int semicolon = queryAndOutput.indexOf(';');
        if (semicolon < 0)
            throw new RuntimeException("Could not parse query and output");
        String query = queryAndOutput.substring(0, semicolon);
        String expected = queryAndOutput.substring(semicolon + 1);
        this.compare(query, expected, true);
        this.compare(query, expected, false);
    }

    /**
     * Test a sequence of queries, each followed by its expected output.
     * Two queries are separated by a whitespace line.
     * Here is an example legal input:
     * SELECT f.* FROM FLOAT4_TBL f WHERE f.f1 = '1004.3';
     *    f1
     * --------
     *  1004.3
     * (1 row)
     *
     * SELECT f.* FROM FLOAT4_TBL f WHERE '1004.3' > f.f1;
     *       f1
     * ---------------
     *              0
     *         -34.84
     *  1.2345679e-20
     * (3 rows)
     */
    public void qs(String queriesWithOutputs) {
        String[] parts = queriesWithOutputs.split("\n\n");
        // From each part drop the last line (N rows) *and* its last newline.
        Pattern regex = Pattern.compile("^(.*)\\n\\(\\d+ rows\\)$", Pattern.DOTALL);
        for (String part: parts) {
            Matcher regexMatcher = regex.matcher(part);
            if (regexMatcher.find()) {
                String result = regexMatcher.group(1);
                this.q(result);
            }
        }
    }
}
