/*
 * Copyright 2022 VMware, Inc.
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

package org.dbsp.sqlCompiler.compiler;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.apache.calcite.adapter.jdbc.JdbcSchema;
import org.apache.calcite.jdbc.CalciteConnection;
import org.apache.calcite.rel.RelNode;
import org.apache.calcite.schema.SchemaPlus;
import org.apache.calcite.tools.FrameworkConfig;
import org.apache.calcite.tools.Frameworks;
import org.apache.calcite.tools.RelBuilder;
import org.apache.calcite.tools.RelRunner;
import org.dbsp.sqlCompiler.compiler.backend.rust.RustFileWriter;
import org.dbsp.sqlCompiler.compiler.errors.CompilerMessages;
import org.dbsp.sqlCompiler.CompilerMain;
import org.dbsp.sqlCompiler.circuit.DBSPCircuit;
import org.dbsp.sqlCompiler.compiler.backend.DBSPCompiler;
import org.dbsp.sqlCompiler.compiler.backend.ToCsvVisitor;
import org.dbsp.sqlCompiler.compiler.backend.rust.ToRustVisitor;
import org.dbsp.sqlCompiler.compiler.frontend.CalciteObject;
import org.dbsp.sqlCompiler.compiler.frontend.CollectIdentifiers;
import org.dbsp.sqlCompiler.compiler.frontend.calciteCompiler.CalciteCompiler;
import org.dbsp.sqlCompiler.compiler.visitors.outer.Passes;
import org.dbsp.sqlCompiler.ir.DBSPFunction;
import org.dbsp.sqlCompiler.ir.DBSPNode;
import org.dbsp.sqlCompiler.ir.expression.*;
import org.dbsp.sqlCompiler.ir.expression.literal.*;
import org.dbsp.sqlCompiler.ir.statement.DBSPExpressionStatement;
import org.dbsp.sqlCompiler.ir.statement.DBSPLetStatement;
import org.dbsp.sqlCompiler.ir.statement.DBSPStatement;
import org.dbsp.sqlCompiler.ir.type.DBSPTypeUser;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeInteger;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeVoid;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeWeight;
import org.dbsp.util.FreshName;
import org.dbsp.util.IWritesLogs;
import org.dbsp.util.Linq;
import org.dbsp.util.Logger;
import org.dbsp.util.NameGen;
import org.dbsp.util.StringPrintStream;
import org.dbsp.util.Utilities;
import org.junit.Assert;
import org.junit.Ignore;
import org.junit.Test;

import javax.imageio.ImageIO;
import javax.sql.DataSource;
import java.io.*;
import java.nio.file.Files;
import java.sql.*;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashSet;
import java.util.List;
import java.util.Objects;
import java.util.Set;

import static org.dbsp.sqlCompiler.ir.type.DBSPTypeCode.USER;

public class OtherTests extends BaseSQLTests implements IWritesLogs {
    static CompilerOptions getOptions() {
        CompilerOptions options = new CompilerOptions();
        options.optimizerOptions.throwOnError = true;
        return options;
    }

    private DBSPCompiler compileDef() {
        DBSPCompiler compiler = new DBSPCompiler(getOptions());
        String ddl = "CREATE TABLE T (\n" +
                "COL1 INT NOT NULL" +
                ", COL2 DOUBLE NOT NULL" +
                ", COL3 BOOLEAN NOT NULL" +
                ", COL4 VARCHAR NOT NULL" +
                ", COL5 INT" +
                ", COL6 DOUBLE" +
                ")";

        compiler.compileStatement(ddl);
        return compiler;
    }

    @Test
    public void toRustTest() {
        String query = "SELECT T.COL3 FROM T";
        query = "CREATE VIEW V AS " + query;
        DBSPCompiler compiler = this.compileDef();
        compiler.compileStatement(query);
        DBSPCircuit circuit = getCircuit(compiler);
        String rust = ToRustVisitor.toRustString(compiler, circuit);
        Assert.assertNotNull(rust);
    }

    @Test
    // This is also testing the deterministic node numbering
    // The numbering of the nodes will change when the optimizations are changed.
    public void toStringTest() {
        NameGen.reset();
        DBSPNode.reset();
        String query = "CREATE VIEW V AS SELECT T.COL3 FROM T";
        DBSPCompiler compiler = this.compileDef();
        compiler.compileStatement(query);
        DBSPCircuit circuit = getCircuit(compiler);
        String str = circuit.toString();
        String expected = "Circuit circuit0 {\n" +
                "    // DBSPSourceOperator 15\n" +
                "    // CREATE TABLE T (\n" +
                "    // COL1 INT NOT NULL, COL2 DOUBLE NOT NULL, COL3 BOOLEAN NOT NULL, COL4 VARCHAR NOT NULL, COL5 INT, COL6 DOUBLE)\n" +
                "    let T = T();\n" +
                "    // DBSPMapOperator 63\n" +
                "    let stream1: stream<OrdZSet<Tuple1<b>, Weight>> = T.map((|t: &Tuple6<i32, d, b, s, i32?, d?>| Tuple1::new((t.2))));\n" +
                "    // CREATE VIEW V AS SELECT T.COL3 FROM T\n" +
                "    // DBSPSinkOperator 67\n" +
                "    let V: stream<OrdZSet<Tuple1<b>, Weight>> = stream1;\n" +
                "}\n";
        Assert.assertEquals(expected, str);
    }

    // Test the ability to redirect logging streams.
    @Test
    public void loggerTest() {
        StringBuilder builder = new StringBuilder();
        Appendable save = Logger.INSTANCE.setDebugStream(builder);
        Logger.INSTANCE.setLoggingLevel(this.getClassName(), 1);
        Assert.assertEquals("OtherTests", this.getClassName());
        Logger.INSTANCE.belowLevel(this, 1)
                .append("Logging one statement")
                .newline();
        Logger.INSTANCE.setLoggingLevel(this.getClassName(), 0);
        Logger.INSTANCE.belowLevel(this, 1)
                .append("This one is not logged")
                .newline();
        Logger.INSTANCE.setDebugStream(save);
        Assert.assertEquals("Logging one statement\n", builder.toString());
        Logger.INSTANCE.setLoggingLevel(this.getClassName(), 0);
    }

    // Test the -T command-line parameter
    @Test
    public void loggingParameter() throws IOException, InterruptedException {
        StringBuilder builder = new StringBuilder();
        Appendable save = Logger.INSTANCE.setDebugStream(builder);
        String[] statements = new String[]{
                "CREATE TABLE T (\n" +
                        "COL1 INT NOT NULL" +
                        ", COL2 DOUBLE NOT NULL" +
                        ")",
                "CREATE VIEW V AS SELECT COL1 FROM T"
        };
        File file = this.createInputScript(statements);
        CompilerMain.execute("-TCalciteCompiler=2", "-TPasses=2",
                "-o", BaseSQLTests.testFilePath, file.getPath());
        Utilities.compileAndTestRust(BaseSQLTests.rustDirectory, true);
        boolean success = file.delete();
        Assert.assertTrue(success);
        Logger.INSTANCE.setDebugStream(save);
        String messages = builder.toString();
        Assert.assertTrue(messages.contains("After optimizer"));
        Assert.assertTrue(messages.contains("CircuitRewriter:Simplify"));
        Logger.INSTANCE.setLoggingLevel(CalciteCompiler.class, 0);
        Logger.INSTANCE.setLoggingLevel(Passes.class, 0);
    }

    @Test
    public void testJoin() throws IOException, InterruptedException {
        String statement0 = "CREATE TABLE demographics (\n" +
                "    cc_num FLOAT64,\n" +
                "    first STRING,\n" +
                "    gender STRING,\n" +
                "    street STRING,\n" +
                "    city STRING,\n" +
                "    state STRING,\n" +
                "    zip INTEGER,\n" +
                "    lat FLOAT64,\n" +
                "    long FLOAT64,\n" +
                "    city_pop INTEGER,\n" +
                "    job STRING,\n" +
                "    dob DATE\n" +
                ")\n";
        String statement1 =
                "CREATE TABLE transactions (\n" +
                "    trans_date_trans_time TIMESTAMP NOT NULL,\n" +
                "    cc_num FLOAT64,\n" +
                "    merchant STRING,\n" +
                "    category STRING,\n" +
                "    amt FLOAT64,\n" +
                "    trans_num STRING,\n" +
                "    unix_time INTEGER,\n" +
                "    merch_lat FLOAT64,\n" +
                "    merch_long FLOAT64,\n" +
                "    is_fraud INTEGER\n" +
                ")\n";
        String statement2 =
                "CREATE VIEW transactions_with_demographics as \n" +
                "    SELECT transactions.*, demographics.first, demographics.city\n" +
                "    FROM\n" +
                "        transactions JOIN demographics\n" +
                "        ON transactions.cc_num = demographics.cc_num";
        DBSPCompiler compiler = new DBSPCompiler(getOptions());
        compiler.compileStatement(statement0);
        compiler.compileStatement(statement1);
        compiler.compileStatement(statement2);
        DBSPCircuit circuit = getCircuit(compiler);
        RustFileWriter writer = new RustFileWriter(compiler, testFilePath);
        writer.add(circuit);
        writer.writeAndClose();
        Utilities.compileAndTestRust(rustDirectory, true);
    }

    @Test
    public void toCsvTest() {
        DBSPCompiler compiler = testCompiler();
        DBSPZSetLiteral s = new DBSPZSetLiteral(DBSPTypeWeight.INSTANCE, EndToEndTests.e0, EndToEndTests.e1);
        StringBuilder builder = new StringBuilder();
        ToCsvVisitor visitor = new ToCsvVisitor(compiler, builder, () -> "");
        visitor.traverse(s);
        String[] lines = builder.toString().split("\n");
        Arrays.sort(lines);
        Assert.assertEquals(
                "10,1.0,false,\"Hi\",1,0.0,\n" +
                "10,12.0,true,\"Hi\",,,",
                String.join("\n", lines));
    }

    @Test
    public void rustCsvTest() throws IOException, InterruptedException {
        DBSPCompiler compiler = testCompiler();
        DBSPZSetLiteral data = new DBSPZSetLiteral(DBSPTypeWeight.INSTANCE, EndToEndTests.e0, EndToEndTests.e1);
        String fileName = BaseSQLTests.rustDirectory + "/" + "test.csv";
        File file = ToCsvVisitor.toCsv(compiler, fileName, data);
        List<DBSPStatement> list = new ArrayList<>();
        // let src = csv_source::<Tuple3<bool, Option<String>, Option<u32>>, isize>("src/test.csv");
        DBSPLetStatement src = new DBSPLetStatement("src",
                new DBSPApplyExpression("read_csv", data.getType(),
                        new DBSPStrLiteral(fileName)));
        list.add(src);
        list.add(new DBSPExpressionStatement(new DBSPApplyExpression(
                "assert_eq!", DBSPTypeVoid.INSTANCE, src.getVarReference(),
                data)));
        DBSPExpression body = new DBSPBlockExpression(list, null);
        DBSPFunction tester = new DBSPFunction("test", new ArrayList<>(),
                DBSPTypeVoid.INSTANCE, body, Linq.list("#[test]"));

        RustFileWriter writer = new RustFileWriter(compiler, BaseSQLTests.testFilePath);
        writer.add(tester);
        writer.writeAndClose();
        Utilities.compileAndTestRust(BaseSQLTests.rustDirectory, false);
        boolean success = file.delete();
        Assert.assertTrue(success);
    }

    @SuppressWarnings("SqlDialectInspection")
    @Test
    public void rustSqlTest() throws IOException, InterruptedException, SQLException {
        String filepath = BaseSQLTests.rustDirectory + "/" + "test.db";
        Connection connection = DriverManager.getConnection("jdbc:sqlite:" + filepath);
        Statement statement = connection.createStatement();
        statement.executeUpdate("drop table if exists t1");
        statement.executeUpdate("create table t1(c1 integer not null, c2 bool not null, " +
                                "c3 varcharnot null , c4 integer)");
        statement.executeUpdate("insert into t1 values(10, true, 'Hi', null)"); // e0NoDouble
        statement.executeUpdate("insert into t1 values(10, false, 'Hi', 1)"); // e1NoDouble
        connection.close();
        DBSPCompiler compiler = testCompiler();

        DBSPZSetLiteral data = new DBSPZSetLiteral(
                DBSPTypeWeight.INSTANCE, EndToEndTests.e0NoDouble, EndToEndTests.e1NoDouble);
        List<DBSPStatement> list = new ArrayList<>();

        String connectionString = "sqlite://" + filepath;
        // Generates a read_table(<conn>, <table_name>, <mapper from |AnyRow| -> Tuple type>) invocation
        DBSPTypeUser sqliteRowType = new DBSPTypeUser(CalciteObject.EMPTY, USER, "AnyRow", false);
        DBSPVariablePath rowVariable = new DBSPVariablePath("row", sqliteRowType);
        DBSPExpression[] fields = EndToEndTests.e0NoDouble.fields; // Should be the same for e1NoDouble too
        final List<DBSPExpression> rowGets = new ArrayList<>(fields.length);
        for (int i = 0; i < fields.length; i++) {
            DBSPApplyMethodExpression rowGet =
                    new DBSPApplyMethodExpression("get",
                            fields[i].getType(),
                            rowVariable, new DBSPUSizeLiteral(i));
            rowGets.add(rowGet);
        }
        DBSPTupleExpression tuple = new DBSPTupleExpression(rowGets, false);
        DBSPClosureExpression mapClosure = new DBSPClosureExpression(tuple,
               rowVariable.asRefParameter());
        DBSPApplyExpression readDb = new DBSPApplyExpression("read_db", data.getType(),
                new DBSPStrLiteral(connectionString), new DBSPStrLiteral("t1"), mapClosure);

        DBSPLetStatement src = new DBSPLetStatement("src", readDb);
        list.add(src);
        list.add(new DBSPExpressionStatement(new DBSPApplyExpression(
                "assert_eq!", DBSPTypeVoid.INSTANCE, src.getVarReference(),
                data)));
        DBSPExpression body = new DBSPBlockExpression(list, null);
        DBSPFunction tester = new DBSPFunction("test", new ArrayList<>(),
                DBSPTypeVoid.INSTANCE, body, Linq.list("#[test]"));

        RustFileWriter writer = new RustFileWriter(compiler, BaseSQLTests.testFilePath);
        writer.add(tester);
        writer.writeAndClose();
        Utilities.compileAndTestRust(BaseSQLTests.rustDirectory, false);
        boolean success = new File(filepath).delete();
        Assert.assertTrue(success);
    }

    @Test
    public void rustCsvTest2() throws IOException, InterruptedException {
        DBSPCompiler compiler = testCompiler();
        DBSPZSetLiteral data = new DBSPZSetLiteral(
                DBSPTypeWeight.INSTANCE,
                new DBSPTupleExpression(new DBSPI32Literal(1, true)),
                new DBSPTupleExpression(new DBSPI32Literal(2, true)),
                new DBSPTupleExpression(DBSPI32Literal.none(DBSPTypeInteger.NULLABLE_SIGNED_32))
        );
        String fileName = BaseSQLTests.rustDirectory + "/" + "test.csv";
        File file = ToCsvVisitor.toCsv(compiler, fileName, data);
        List<DBSPStatement> list = new ArrayList<>();
        DBSPLetStatement src = new DBSPLetStatement("src",
                new DBSPApplyExpression("read_csv", data.getType(),
                        new DBSPStrLiteral(fileName)));
        list.add(src);
        list.add(new DBSPExpressionStatement(new DBSPApplyExpression(
                "assert_eq!", DBSPTypeVoid.INSTANCE, src.getVarReference(),
                data)));
        DBSPExpression body = new DBSPBlockExpression(list, null);
        DBSPFunction tester = new DBSPFunction("test", new ArrayList<>(),
                DBSPTypeVoid.INSTANCE, body, Linq.list("#[test]"));

        PrintStream outputStream = new PrintStream(BaseSQLTests.testFilePath, "UTF-8");
        RustFileWriter writer = new RustFileWriter(compiler, outputStream);
        writer.add(tester);
        writer.writeAndClose();
        Utilities.compileAndTestRust(BaseSQLTests.rustDirectory, false);
        boolean success = file.delete();
        Assert.assertTrue(success);
    }

    File createInputScript(String... contents) throws FileNotFoundException, UnsupportedEncodingException {
        String inputScript = rustDirectory + "/script.sql";
        PrintWriter script = new PrintWriter(inputScript, "UTF-8");
        script.println(String.join(";\n", contents));
        script.close();
        return new File(inputScript);
    }

    @Test
    public void testProjectFiles() {
        // Compiles all the programs in the tests directory
        final String projectsDirectory = "../../demo/";
        File dir = new File(projectsDirectory);
        File[] subdirs = dir.listFiles(File::isDirectory);
        Objects.requireNonNull(subdirs);
        for (File subdir: subdirs) {
            if (!subdir.getName().contains("project_"))
                continue;
            String path = subdir.getPath() + "/project.sql";
            CompilerMessages messages = CompilerMain.execute("-o", "/dev/null", path);
            System.out.println(messages);
            Assert.assertEquals(0, messages.errorCount());
        }
    }

    @Test
    public void testRustCompiler() throws IOException, InterruptedException {
        String[] statements = new String[]{
                "CREATE TABLE T (\n" +
                        "COL1 INT NOT NULL" +
                        ", COL2 DOUBLE NOT NULL" +
                        ")",
                "CREATE VIEW V AS SELECT COL1 FROM T"
        };
        File file = this.createInputScript(statements);
        CompilerMain.execute("-o", BaseSQLTests.testFilePath, file.getPath());
        Utilities.compileAndTestRust(BaseSQLTests.rustDirectory, false);
        boolean success = file.delete();
        Assert.assertTrue(success);
    }
    
    @Test
    public void testSchema() throws IOException {
        String[] statements = new String[]{
                "CREATE TABLE T (\n" +
                        "COL1 INT NOT NULL" +
                        ", COL2 DOUBLE NOT NULL" +
                        ")",
                "CREATE VIEW V AS SELECT COL1 FROM T"
        };
        File file = this.createInputScript(statements);
        File json = File.createTempFile("out", ".json", new File("."));
        File tmp = File.createTempFile("out", ".rs", new File("."));
        CompilerMessages message = CompilerMain.execute(
                "-js", json.getPath(), "-o", tmp.getPath(), file.getPath());
        Assert.assertEquals(message.exitCode, 0);
        ObjectMapper mapper = new ObjectMapper();
        JsonNode parsed = mapper.readTree(json);
        Assert.assertNotNull(parsed);
        boolean success = file.delete();
        Assert.assertTrue(success);
        success = json.delete();
        Assert.assertTrue(success);
        success = tmp.delete();
        Assert.assertTrue(success);
    }

    @Test @Ignore("Only run if we want to preserve casing for names")
    public void testCaseSensitive() throws IOException {
        String[] statements = new String[]{
                "CREATE TABLE MYTABLE (\n" +
                        "COL1 INT NOT NULL" +
                        ", COL2 DOUBLE NOT NULL" +
                        ")",
                "CREATE VIEW V AS SELECT COL1 FROM MYTABLE",
                "CREATE TABLE yourtable (\n" +
                        "  column1 INT NOT NULL" +
                        ", column2 DOUBLE NOT NULL" +
                        ")",
                "CREATE VIEW V2 AS SELECT column2 FROM yourtable"
        };
        File file = this.createInputScript(statements);
        File json = File.createTempFile("out", ".json", new File("."));
        json.deleteOnExit();
        File tmp = File.createTempFile("out", ".rs", new File("."));
        tmp.deleteOnExit();
        CompilerMessages message = CompilerMain.execute(
                "-js", json.getPath(), "-o", tmp.getPath(), file.getPath());
        Assert.assertEquals(message.exitCode, 0);
        ObjectMapper mapper = new ObjectMapper();
        JsonNode parsed = mapper.readTree(json);
        Assert.assertNotNull(parsed);
        String jsonContents  = String.join("\n", Files.readAllLines(json.toPath()));
        System.out.println(jsonContents);
        Assert.assertTrue(jsonContents.contains("MYTABLE"));  // checks case sensitivity
        Assert.assertTrue(jsonContents.contains("COL1"));
        Assert.assertTrue(jsonContents.contains("yourtable"));
        Assert.assertTrue(jsonContents.contains("column1"));
        boolean success = file.delete();
        Assert.assertTrue(success);
    }

    @Test
    public void testCompilerToJson() throws IOException {
        String[] statements = new String[]{
                "CREATE TABLE T (\n" +
                        "COL1 INT NOT NULL" +
                        ", COL2 DOUBLE NOT NULL" +
                        ")",
                "CREATE VIEW V AS SELECT COL1 FROM T WHERE COL1 > 5"
        };
        File file = this.createInputScript(statements);
        File json = File.createTempFile("out", ".json", new File("."));
        CompilerMessages message = CompilerMain.execute("-j", "-o", json.getPath(), file.getPath());
        Assert.assertEquals(message.exitCode, 0);
        ObjectMapper mapper = new ObjectMapper();
        JsonNode parsed = mapper.readTree(json);
        Assert.assertNotNull(parsed);
        boolean success = file.delete();
        Assert.assertTrue(success);
        success = json.delete();
        Assert.assertTrue(success);
    }

    @Test
    public void testCompilerToPng() throws IOException {
        String[] statements = new String[]{
                "CREATE TABLE T (\n" +
                        "COL1 INT NOT NULL" +
                        ", COL2 DOUBLE NOT NULL" +
                        ")",
                "CREATE VIEW V AS SELECT COL1 FROM T WHERE COL1 > 5"
        };
        File file = this.createInputScript(statements);
        File png = File.createTempFile("out", ".png", new File("."));
        png.deleteOnExit();
        CompilerMessages message = CompilerMain.execute("-png", "-o", png.getPath(), file.getPath());
        Assert.assertEquals(message.exitCode, 0);
        Assert.assertTrue(file.exists());
        ImageIO.read(new File(png.getPath()));
        boolean success = file.delete();
        Assert.assertTrue(success);
    }

    @Test
    public void compilerError() throws FileNotFoundException, UnsupportedEncodingException {
        String statement = "CREATE TABLE T (\n" +
                "  COL1 INT NOT NULL" +
                ", COL2 GARBAGE";
        File file = this.createInputScript(statement);
        CompilerMessages messages = CompilerMain.execute(file.getPath(), "-o", "/dev/null");
        Assert.assertEquals(messages.exitCode, 1);
        Assert.assertEquals(messages.errorCount(), 1);
        CompilerMessages.Error error = messages.messages.get(0);
        Assert.assertTrue(error.message.startsWith("Encountered \"<EOF>\""));
    }

    @Test
    public void warningTest() throws FileNotFoundException, UnsupportedEncodingException {
        String statements = "CREATE TABLE T (COL1 INT);\n" +
                "CREATE TABLE S (COL1 INT);\n" +
                "CREATE VIEW V AS SELECT * FROM S";
        File file = this.createInputScript(statements);
        CompilerMessages messages = CompilerMain.execute(file.getPath(), "-o", "/dev/null", "-alltables");
        Assert.assertEquals(messages.exitCode, 0);
        Assert.assertEquals(messages.warningCount(), 1);
        Assert.assertEquals(messages.errorCount(), 0);
        CompilerMessages.Error error = messages.messages.get(0);
        Assert.assertTrue(error.warning);
        Assert.assertTrue(error.message.contains("Table 'T' is not used"));
    }

    @Test
    public void errorTest() throws FileNotFoundException, UnsupportedEncodingException {
        String[] statements = new String[]{
                "This is not SQL"
        };
        File file = this.createInputScript(statements);
        CompilerMessages messages = CompilerMain.execute("-o", BaseSQLTests.testFilePath, file.getPath());
        Assert.assertEquals(messages.exitCode, 1);
        Assert.assertEquals(messages.errorCount(), 1);
        CompilerMessages.Error msg = messages.getError(0);
        Assert.assertFalse(msg.warning);
        Assert.assertEquals(msg.message, "Non-query expression encountered in illegal context");

        statements = new String[] {
                "CREATE VIEW V AS SELECT * FROM T"
        };
        file = this.createInputScript(statements);
        messages = CompilerMain.execute("-o", BaseSQLTests.testFilePath, file.getPath());
        Assert.assertEquals(messages.exitCode, 1);
        Assert.assertEquals(messages.errorCount(), 1);
        msg = messages.getError(0);
        Assert.assertFalse(msg.warning);
        Assert.assertEquals(msg.message, "Object 'T' not found");

        statements = new String[] {
                "CREATE VIEW V AS SELECT ST_MAKELINE(ST_POINT(0,0), ST_POINT(0, 0))"
        };
        file = this.createInputScript(statements);
        messages = CompilerMain.execute("-o", BaseSQLTests.testFilePath, file.getPath());
        Assert.assertEquals(messages.exitCode, 1);
        Assert.assertEquals(messages.errorCount(), 1);
        msg = messages.getError(0);
        Assert.assertFalse(msg.warning);
        Assert.assertEquals(msg.message, "Not yet implemented: cannot convert GEOMETRY literal to class org.locationtech.jts.geom.Point\n" +
                "LINESTRING (0 0, 0 0):GEOMETRY");

        boolean success = file.delete();
        Assert.assertTrue(success);
    }

    @Test
    public void testFreshName() {
        String query = "CREATE VIEW V AS SELECT T.COL1 FROM T WHERE T.COL2 > 0";
        DBSPCompiler compiler = this.compileDef();
        compiler.compileStatement(query);
        DBSPCircuit circuit = getCircuit(compiler);
        Set<String> used = new HashSet<>();
        CollectIdentifiers ci = new CollectIdentifiers(testCompiler(), used);
        ci.getCircuitVisitor().apply(circuit);
        Assert.assertTrue(used.contains("T")); // table name
        Assert.assertTrue(used.contains("V")); // view name
        FreshName gen = new FreshName(used);
        String t0 = gen.freshName("T");
        Assert.assertEquals(t0, "T_0");
        String t1 = gen.freshName("T");
        Assert.assertEquals(t1, "T_1");
    }

    @Test
    public void jsonErrorTest() throws FileNotFoundException, UnsupportedEncodingException, JsonProcessingException {
        String[] statements = new String[] {
                "CREATE VIEW V AS SELECT * FROM T"
        };
        File file = this.createInputScript(statements);
        CompilerMessages messages = CompilerMain.execute("-je", file.getPath());
        Assert.assertEquals(messages.exitCode, 1);
        Assert.assertEquals(messages.errorCount(), 1);
        String json = messages.toString();
        ObjectMapper mapper = new ObjectMapper();
        JsonNode jsonNode = mapper.readTree(json);
        Assert.assertNotNull(jsonNode);
    }

    @SuppressWarnings("SqlDialectInspection")
    @Test
    public void HSQLDBTest() throws SQLException {
        String jdbcUrl = "jdbc:hsqldb:mem:db";
        Connection connection = DriverManager.getConnection(jdbcUrl, "", "");
        try (Statement s = connection.createStatement()) {
            s.execute("create table mytable(" +
                    "id integer not null primary key," +
                    "strcol varchar(25))");

            s.execute("insert into mytable values(0, 'str0')");
            s.execute("insert into mytable values(1, 'str1')");
        }

        StringPrintStream direct = new StringPrintStream();
        try (Statement s = connection.createStatement()) {
            ResultSet resultSet = s.executeQuery("SELECT * FROM mytable WHERE id > 0");
            Utilities.showResultSet(resultSet, direct.getPrintStream());
        }

        DataSource mockDataSource = JdbcSchema.dataSource(jdbcUrl, "org.hsqldb.jdbcDriver", "", "");
        Connection executorConnection = DriverManager.getConnection("jdbc:calcite:");
        CalciteConnection calciteConnection = executorConnection.unwrap(CalciteConnection.class);
        SchemaPlus rootSchema = calciteConnection.getRootSchema();
        rootSchema.add("schema", JdbcSchema.create(rootSchema, "schema", mockDataSource, null, null));

        FrameworkConfig config = Frameworks.newConfigBuilder()
                .defaultSchema(rootSchema)
                .build();
        RelBuilder r = RelBuilder.create(config);
        RelNode node = r
                .scan("schema", "MYTABLE")
                .filter(r.equals(r.field("ID"), r.literal(1)))
                .project(
                        r.field("ID"),
                        r.field("STRCOL")
                )
                .build();
        RelRunner runner = calciteConnection.unwrap(RelRunner.class);
        try (PreparedStatement ps = runner.prepareStatement(node)) {
            ps.execute();
            ResultSet resultSet = ps.getResultSet();
            StringPrintStream throughCalcite = new StringPrintStream();
            Utilities.showResultSet(resultSet, throughCalcite.getPrintStream());
            Assert.assertEquals(direct.toString(), throughCalcite.toString());
        }
    }

    @SuppressWarnings("SqlDialectInspection")
    @Test @Ignore("Fails due to a bug in HSQLDB")
    public void HSQLDBDoubleNegTest() throws SQLException {
        // Reproduction for https://sourceforge.net/p/hsqldb/bugs/1680/
        // and https://sourceforge.net/p/hsqldb/bugs/1681/
        String jdbcUrl = "jdbc:hsqldb:mem:db";
        Connection connection = DriverManager.getConnection(jdbcUrl, "", "");
        try (Statement s = connection.createStatement()) {
            s.execute("SELECT +2;");
            s.execute("SELECT - -2;");
        }
    }

    @SuppressWarnings("SqlDialectInspection")
    @Test
    public void rawCalciteTest() throws SQLException {
        Connection connection = DriverManager.getConnection("jdbc:calcite:");
        String query = "SELECT timestampdiff(MONTH, TIMESTAMP'2021-02-28 12:00:00', TIMESTAMP'2021-03-28 11:59:59')";
        try (PreparedStatement ps = connection.prepareStatement(query)) {
            ps.execute();
            try (ResultSet resultSet = ps.getResultSet()) {
                while (resultSet.next()) {
                    int result = resultSet.getInt(1);
                    Assert.assertEquals(0, result);
                }
            }
        }
    }
}
