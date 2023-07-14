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

import org.dbsp.sqlCompiler.circuit.DBSPCircuit;
import org.dbsp.sqlCompiler.compiler.backend.DBSPCompiler;
import org.dbsp.sqlCompiler.compiler.backend.rust.RustFileWriter;
import org.dbsp.sqlCompiler.ir.expression.DBSPTupleExpression;
import org.dbsp.sqlCompiler.ir.expression.literal.*;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeDouble;
import org.dbsp.util.Utilities;
import org.junit.Assert;
import org.junit.Test;

import java.io.IOException;

@SuppressWarnings("SpellCheckingInspection")
public class ComplexQueriesTest extends BaseSQLTests {
    @Test
    public void smallTaxiTest() {
        String ddl = "CREATE TABLE green_tripdata\n" +
                "(\n" +
                "  lpep_pickup_datetime TIMESTAMP NOT NULL,\n" +
                "  lpep_dropoff_datetime TIMESTAMP NOT NULL,\n" +
                "  pickup_location_id BIGINT NOT NULL,\n" +
                "  dropoff_location_id BIGINT NOT NULL,\n" +
                "  trip_distance DOUBLE PRECISION,\n" +
                "  fare_amount DOUBLE PRECISION \n" +
                ")";
        String query =
                "SELECT\n" +
                        "*,\n" +
                        "COUNT(*) OVER(\n" +
                        "   PARTITION BY  pickup_location_id\n" +
                        "   ORDER BY  extract (EPOCH from  CAST (lpep_pickup_datetime AS TIMESTAMP) ) \n" +
                        "   -- 1 hour is 3600  seconds\n" +
                        "   RANGE BETWEEN 3600  PRECEDING AND 1 PRECEDING ) AS count_trips_window_1h_pickup_zip\n" +
                        "FROM green_tripdata";
        DBSPCompiler compiler = testCompiler();
        query = "CREATE VIEW V AS (" + query + ")";
        compiler.compileStatement(ddl);
        compiler.compileStatement(query);
        Assert.assertFalse(compiler.hasErrors());
        this.addRustTestCase("ComplexQueriesTest.smallTaxiTest", compiler, getCircuit(compiler));
    }

    // Also compiles the code using the Rust handle API
    @Test
    public void testComplex() throws IOException, InterruptedException {
        String statements = "-- Git repository.\n" +
                "create table repository (\n" +
                "    repository_id bigint not null,\n" +
                "    type varchar not null,\n" +
                "    url varchar not null,\n" +
                "    name varchar not null\n" +
                ");\n" +
                "\n" +
                "-- Commit inside a Git repo.\n" +
                "create table git_commit (\n" +
                "    git_commit_id bigint not null,\n" +
                "    repository_id bigint not null,\n" +
                "    commit_id varchar not null,\n" +
                "    commit_date timestamp not null,\n" +
                "    commit_owner varchar not null\n" +
                ");\n" +
                "\n" +
                "-- CI pipeline.\n" +
                "create table pipeline (\n" +
                "    pipeline_id bigint not null,\n" +
                "    name varchar not null,\n" +
                "    create_date timestamp not null,\n" +
                "    createdby_user_id bigint not null,\n" +
                "    update_date timestamp,\n" +
                "    updatedby_user_id bigint\n" +
                ");\n" +
                "\n" +
                "\n" +
                "-- Git commits used by each pipeline.\n" +
                "create table pipeline_sources (\n" +
                "    git_commit_id bigint not null,\n" +
                "    pipeline_id bigint not null\n" +
                ");\n" +
                "\n" +
                "-- Binary artifact created by a CI pipeline.\n" +
                "create table artifact (\n" +
                "    artifact_id bigint not null,\n" +
                "    artifact_uri varchar not null,\n" +
                "    path varchar not null,\n" +
                "    create_date timestamp not null,\n" +
                "    createdby_user_id bigint not null,\n" +
                "    update_date timestamp,\n" +
                "    updatedby_user_id bigint,\n" +
                "    checksum varchar not null,\n" +
                "    checksum_type varchar not null,\n" +
                "    artifact_size_in_bytes bigint not null,\n" +
                "    artifact_type varchar not null,\n" +
                "    builtby_pipeline_id bigint not null,\n" +
                "    parent_artifact_id bigint\n" +
                ");\n" +
                "\n" +
                "-- Vulnerabilities discovered in source code.\n" +
                "create table vulnerability (\n" +
                "    vulnerability_id bigint not null,\n" +
                "    discovery_date timestamp not null,\n" +
                "    discovered_by varchar not null,\n" +
                "    discovered_in bigint not null /*git_commit_id*/,\n" +
                "    update_date timestamp,\n" +
                "    updatedby_user_id bigint,\n" +
                "    checksum varchar not null,\n" +
                "    checksum_type varchar not null,\n" +
                "    vulnerability_reference_id varchar not null,\n" +
                "    severity varchar,\n" +
                "    priority varchar\n" +
                ");\n" +
                "\n" +
                "-- Deployed k8s objects.\n" +
                "create table k8sobject (\n" +
                "    k8sobject_id bigint not null,\n" +
                "    create_date timestamp not null,\n" +
                "    createdby_user_id bigint not null,\n" +
                "    update_date timestamp,\n" +
                "    updatedby_user_id bigint,\n" +
                "    checksum varchar not null,\n" +
                "    checksum_type varchar not null,\n" +
                "    deployed_id bigint not null /*k8scluster_id*/,\n" +
                "    deployment_type varchar not null,\n" +
                "    k8snamespace varchar not null\n" +
                ");\n" +
                "\n" +
                "-- Binary artifacts used to construct k8s objects.\n" +
                "create table k8sartifact (\n" +
                "    artifact_id bigint not null,\n" +
                "    k8sobject_id bigint not null\n" +
                ");\n" +
                "\n" +
                "-- K8s clusters.\n" +
                "create table k8scluster (\n" +
                "    k8scluster_id bigint not null,\n" +
                "    k8s_uri varchar not null,\n" +
                "    path varchar not null,\n" +
                "    name varchar not null,\n" +
                "    k8s_serivce_provider varchar not null\n" +
                ");\n" +
                "\n" +
                "create view pipeline_vulnerability (\n" +
                "    pipeline_id,\n" +
                "    vulnerability_id\n" +
                ") as\n" +
                "    SELECT pipeline_sources.pipeline_id as pipeline_id, vulnerability.vulnerability_id as vulnerability_id FROM\n" +
                "    pipeline_sources\n" +
                "    INNER JOIN\n" +
                "    vulnerability\n" +
                "    ON pipeline_sources.git_commit_id = vulnerability.discovered_in;\n" +
                "\n" +
                "create view artifact_vulnerability (\n" +
                "    artifact_id,\n" +
                "    vulnerability_id\n" +
                ") as\n" +
                "    SELECT artifact.artifact_id as artifact_id, pipeline_vulnerability.vulnerability_id as vulnerability_id FROM\n" +
                "    artifact\n" +
                "    INNER JOIN\n" +
                "    pipeline_vulnerability\n" +
                "    ON artifact.builtby_pipeline_id = pipeline_vulnerability.pipeline_id;\n" +
                "\n" +
                "-- Vulnerabilities that could propagate to each artifact.\n" +
                "-- create view artifact_vulnerability ();\n" +
                "\n" +
                "-- Vulnerabilities in the artifact or any of its children.\n" +
                "-- create view transitive_vulnerability ();\n" +
                "\n" +
                "-- create view k8sobject_vulnerability ();\n" +
                "\n" +
                "-- create view k8scluster_vulnerability ();\n" +
                "\n" +
                "-- Number of vulnerabilities.\n" +
                "-- Most severe vulnerability.\n" +
                "-- create view k8scluster_vulnerability_stats ();";
        DBSPCompiler compiler = testCompiler();
        compiler.compileStatements(statements);
        DBSPCircuit circuit = getCircuit(compiler);
        RustFileWriter writer = new RustFileWriter(compiler, testFilePath);
        writer.emitCodeWithHandle(true);
        writer.add(circuit);
        writer.writeAndClose();
        Utilities.compileAndTestRust(rustDirectory, true);
    }

    @Test
    public void testComplex1() throws IOException, InterruptedException {
        String statements = "-- Git repository.\n" +
                "create table repository (\n" +
                "    repository_id bigint not null,\n" +
                "    type varchar not null,\n" +
                "    url varchar not null,\n" +
                "    name varchar not null\n" +
                ");\n" +
                "\n" +
                "-- Commit inside a Git repo.\n" +
                "create table git_commit (\n" +
                "    git_commit_id bigint not null,\n" +
                "    repository_id bigint not null,\n" +
                "    commit_id varchar not null,\n" +
                "    commit_date timestamp not null,\n" +
                "    commit_owner varchar not null\n" +
                ");\n" +
                "\n" +
                "-- CI pipeline.\n" +
                "create table pipeline (\n" +
                "    pipeline_id bigint not null,\n" +
                "    name varchar not null,\n" +
                "    create_date timestamp not null,\n" +
                "    createdby_user_id bigint not null,\n" +
                "    update_date timestamp,\n" +
                "    updatedby_user_id bigint\n" +
                ");\n" +
                "\n" +
                "\n" +
                "-- Git commits used by each pipeline.\n" +
                "create table pipeline_sources (\n" +
                "    git_commit_id bigint not null,\n" +
                "    pipeline_id bigint not null\n" +
                ");\n" +
                "\n" +
                "-- Binary artifact created by a CI pipeline.\n" +
                "create table artifact (\n" +
                "    artifact_id bigint not null,\n" +
                "    artifact_uri varchar not null,\n" +
                "    path varchar not null,\n" +
                "    create_date timestamp not null,\n" +
                "    createdby_user_id bigint not null,\n" +
                "    update_date timestamp,\n" +
                "    updatedby_user_id bigint,\n" +
                "    checksum varchar not null,\n" +
                "    checksum_type varchar not null,\n" +
                "    artifact_size_in_bytes bigint not null,\n" +
                "    artifact_type varchar not null,\n" +
                "    builtby_pipeline_id bigint not null,\n" +
                "    parent_artifact_id bigint\n" +
                ");\n" +
                "\n" +
                "-- Vulnerabilities discovered in source code.\n" +
                "create table vulnerability (\n" +
                "    vulnerability_id bigint not null,\n" +
                "    discovery_date timestamp not null,\n" +
                "    discovered_by varchar not null,\n" +
                "    discovered_in bigint not null /*git_commit_id*/,\n" +
                "    update_date timestamp,\n" +
                "    updatedby_user_id bigint,\n" +
                "    checksum varchar not null,\n" +
                "    checksum_type varchar not null,\n" +
                "    vulnerability_reference_id varchar not null,\n" +
                "    severity varchar,\n" +
                "    priority varchar\n" +
                ");\n" +
                "\n" +
                "-- Deployed k8s objects.\n" +
                "create table k8sobject (\n" +
                "    k8sobject_id bigint not null,\n" +
                "    create_date timestamp not null,\n" +
                "    createdby_user_id bigint not null,\n" +
                "    update_date timestamp,\n" +
                "    updatedby_user_id bigint,\n" +
                "    checksum varchar not null,\n" +
                "    checksum_type varchar not null,\n" +
                "    deployed_id bigint not null /*k8scluster_id*/,\n" +
                "    deployment_type varchar not null,\n" +
                "    k8snamespace varchar not null\n" +
                ");\n" +
                "\n" +
                "-- Binary artifacts used to construct k8s objects.\n" +
                "create table k8sartifact (\n" +
                "    artifact_id bigint not null,\n" +
                "    k8sobject_id bigint not null\n" +
                ");\n" +
                "\n" +
                "-- K8s clusters.\n" +
                "create table k8scluster (\n" +
                "    k8scluster_id bigint not null,\n" +
                "    k8s_uri varchar not null,\n" +
                "    path varchar not null,\n" +
                "    name varchar not null,\n" +
                "    k8s_serivce_provider varchar not null\n" +
                ");\n" +
                "\n" +
                "-- Vulnerabilities that affect each pipeline.\n" +
                "create view pipeline_vulnerability (\n" +
                "    pipeline_id,\n" +
                "    vulnerability_id\n" +
                ") as\n" +
                "    SELECT pipeline_sources.pipeline_id as pipeline_id, vulnerability.vulnerability_id as vulnerability_id FROM\n" +
                "    pipeline_sources\n" +
                "    INNER JOIN\n" +
                "    vulnerability\n" +
                "    ON pipeline_sources.git_commit_id = vulnerability.discovered_in;\n" +
                "\n" +
                "-- Vulnerabilities that could propagate to each artifact.\n" +
                "create view artifact_vulnerability (\n" +
                "    artifact_id,\n" +
                "    vulnerability_id\n" +
                ") as\n" +
                "    SELECT artifact.artifact_id as artifact_id, pipeline_vulnerability.vulnerability_id as vulnerability_id FROM\n" +
                "    artifact\n" +
                "    INNER JOIN\n" +
                "    pipeline_vulnerability\n" +
                "    ON artifact.builtby_pipeline_id = pipeline_vulnerability.pipeline_id;\n" +
                "\n" +
                "-- Vulnerabilities in the artifact or any of its children.\n" +
                "create view transitive_vulnerability(\n" +
                "    artifact_id,\n" +
                "    via_artifact_id,\n" +
                "    vulnerability_id\n" +
                ") as\n" +
                "    SELECT artifact_id, artifact_id as via_artifact_id, vulnerability_id from artifact_vulnerability\n" +
                "    UNION\n" +
                "    (\n" +
                "        SELECT\n" +
                "            artifact.parent_artifact_id as artifact_id,\n" +
                "            artifact.artifact_id as via_artifact_id,\n" +
                "            artifact_vulnerability.vulnerability_id as vulnerability_id FROM\n" +
                "        artifact\n" +
                "        INNER JOIN\n" +
                "        artifact_vulnerability\n" +
                "        ON artifact.artifact_id = artifact_vulnerability.artifact_id\n" +
                "        WHERE artifact.parent_artifact_id IS NOT NULL\n" +
                "    );\n" +
                "\n" +
                "-- create view k8sobject_vulnerability ();\n" +
                "\n" +
                "-- create view k8scluster_vulnerability ();\n" +
                "\n" +
                "-- Number of vulnerabilities.\n" +
                "-- Most severe vulnerability.\n" +
                "-- create view k8scluster_vulnerability_stats ();";
        DBSPCompiler compiler = testCompiler();
        compiler.compileStatements(statements);
        DBSPCircuit circuit = getCircuit(compiler);
        RustFileWriter writer = new RustFileWriter(compiler, testFilePath);
        writer.emitCodeWithHandle(true);
        writer.add(circuit);
        writer.writeAndClose();
        Utilities.compileAndTestRust(rustDirectory, true);
    }

    @Test
    public void testProducts() {
        String script = "-- create a table\n" +
                "CREATE TABLE Products (\n" +
                "    ProductName VARCHAR NOT NULL,\n" +
                "    Price INT NOT NULL);\n" +
                "-- statements separated by semicolons\n" +
                "-- create a view\n" +
                "CREATE VIEW \"Products Above Average Price\" AS\n" +
                "SELECT ProductName, Price\n" +
                "FROM Products\n" +
                "WHERE Price > (SELECT AVG(Price) FROM Products)\n" +
                "-- no semicolon at end ";
        DBSPCompiler compiler = testCompiler();
        compiler.compileStatements(script);
    }

    @Test
    public void demographicsTest() {
        // TODO: LAG is disabled
        String script =
                "CREATE TABLE demographics (\n" +
                "    cc_num FLOAT64 NOT NULL,\n" +
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
                "    dob STRING\n" +
                ");\n" +
                "\n" +
                "CREATE TABLE transactions (\n" +
                "    trans_date_trans_time TIMESTAMP NOT NULL,\n" +
                "    cc_num FLOAT64 NOT NULL,\n" +
                "    merchant STRING,\n" +
                "    category STRING,\n" +
                "    amt FLOAT64,\n" +
                "    trans_num STRING,\n" +
                "    unix_time INTEGER NOT NULL,\n" +
                "    merch_lat FLOAT64,\n" +
                "    merch_long FLOAT64,\n" +
                "    is_fraud INTEGER\n" +
                ");\n" +
                "\n" +
                "CREATE VIEW features as\n" +
                "    SELECT\n" +
                "        DAYOFWEEK(trans_date_trans_time) AS d,\n" +
                "        TIMESTAMPDIFF(YEAR, trans_date_trans_time, CAST(dob as TIMESTAMP)) AS age,\n" +
                "        ST_DISTANCE(ST_POINT(long,lat), ST_POINT(merch_long,merch_lat)) AS distance,\n" +
                "        -- TIMESTAMPDIFF(MINUTE, trans_date_trans_time, last_txn_date) AS trans_diff,\n" +
                "        AVG(amt) OVER(\n" +
                "            PARTITION BY   CAST(cc_num AS NUMERIC)\n" +
                "            ORDER BY unix_time\n" +
                "            -- 1 week is 604800  seconds\n" +
                "            RANGE BETWEEN 604800  PRECEDING AND 1 PRECEDING) AS\n" +
                "        avg_spend_pw,\n" +
                "        AVG(amt) OVER(\n" +
                "            PARTITION BY  CAST(cc_num AS NUMERIC)\n" +
                "            ORDER BY unix_time\n" +
                "            -- 1 month(30 days) is 2592000 seconds\n" +
                "            RANGE BETWEEN 2592000 PRECEDING AND 1 PRECEDING) AS\n" +
                "        avg_spend_pm,\n" +
                "        COUNT(*) OVER(\n" +
                "            PARTITION BY  CAST(cc_num AS NUMERIC)\n" +
                "            ORDER BY unix_time\n" +
                "            -- 1 day is 86400  seconds\n" +
                "            RANGE BETWEEN 86400  PRECEDING AND 1 PRECEDING ) AS\n" +
                "        trans_freq_24,\n" +
                "        category,\n" +
                "        amt,\n" +
                "        state,\n" +
                "        job,\n" +
                "        unix_time,\n" +
                "        city_pop,\n" +
                "        merchant,\n" +
                "        is_fraud\n" +
                "    FROM (\n" +
                "        SELECT t1.*, t2.*\n" +
                "               -- , LAG(trans_date_trans_time, 1) OVER (PARTITION BY t1.cc_num  ORDER BY trans_date_trans_time ASC) AS last_txn_date\n" +
                "        FROM  transactions AS t1\n" +
                "        LEFT JOIN  demographics AS t2\n" +
                "        ON t1.cc_num = t2.cc_num);";
        DBSPCompiler compiler = testCompiler();
        compiler.compileStatements(script);
        DBSPZSetLiteral.Contents[] inputs = new DBSPZSetLiteral.Contents[] {
                new DBSPZSetLiteral.Contents(new DBSPTupleExpression(
                        new DBSPDoubleLiteral(0.0),
                        new DBSPStringLiteral("First", true),
                        new DBSPStringLiteral("Male", true),
                        new DBSPStringLiteral("Street", true),
                        new DBSPStringLiteral("City", true),
                        new DBSPStringLiteral("State", true),
                        new DBSPI32Literal(94043, true),
                        //new DBSPDoubleLiteral(128.0, true),
                        DBSPLiteral.none(DBSPTypeDouble.NULLABLE_INSTANCE),
                        new DBSPDoubleLiteral(128.0, true),
                        new DBSPI32Literal(100000, true),
                        new DBSPStringLiteral("Job", true),
                        new DBSPStringLiteral("2020-02-20", true)
                        )),
                new DBSPZSetLiteral.Contents(new DBSPTupleExpression(
                        new DBSPTimestampLiteral("2020-02-20 10:00:00", false),
                        new DBSPDoubleLiteral(0.0, false),
                        new DBSPStringLiteral("Merchant", true),
                        new DBSPStringLiteral("Category", true),
                        new DBSPDoubleLiteral(10.0, true),
                        new DBSPStringLiteral("Transnum", true),
                        new DBSPI32Literal(1000),
                        new DBSPDoubleLiteral(128.0, true),
                        new DBSPDoubleLiteral(128.0, true),
                        new DBSPI32Literal(0, true)
                ))
        };
        DBSPZSetLiteral.Contents[] outputs = new DBSPZSetLiteral.Contents[] {};
        InputOutputPair ip = new InputOutputPair(inputs, outputs);
        this.addRustTestCase("ComplexQueriesTest.demographicsTest", compiler, getCircuit(compiler), ip);
    }

    @Test
    public void taxiTest() {
        String ddl = "CREATE TABLE green_tripdata\n" +
                "(\n" +
                "        lpep_pickup_datetime TIMESTAMP NOT NULL,\n" +
                "        lpep_dropoff_datetime TIMESTAMP NOT NULL,\n" +
                "        pickup_location_id BIGINT NOT NULL,\n" +
                "        dropoff_location_id BIGINT NOT NULL,\n" +
                "        trip_distance DOUBLE PRECISION,\n" +
                "        fare_amount DOUBLE PRECISION \n" +
                ")";
        String query =
                "SELECT\n" +
                        "*,\n" +
                        "COUNT(*) OVER(\n" +
                        "                PARTITION BY  pickup_location_id\n" +
                        "                ORDER BY  extract (EPOCH from  CAST (lpep_pickup_datetime AS TIMESTAMP) ) \n" +
                        "                -- 1 hour is 3600  seconds\n" +
                        "                RANGE BETWEEN 3600  PRECEDING AND 1 PRECEDING ) AS count_trips_window_1h_pickup_zip,\n" +
                        "AVG(fare_amount) OVER(\n" +
                        "                PARTITION BY  pickup_location_id\n" +
                        "                ORDER BY  extract (EPOCH from  CAST (lpep_pickup_datetime AS TIMESTAMP) ) \n" +
                        "                -- 1 hour is 3600  seconds\n" +
                        "                RANGE BETWEEN 3600  PRECEDING AND 1 PRECEDING ) AS mean_fare_window_1h_pickup_zip,\n" +
                        "COUNT(*) OVER(\n" +
                        "                PARTITION BY  dropoff_location_id\n" +
                        "                ORDER BY  extract (EPOCH from  CAST (lpep_dropoff_datetime AS TIMESTAMP) ) \n" +
                        "                -- 0.5 hour is 1800  seconds\n" +
                        "                RANGE BETWEEN 1800  PRECEDING AND 1 PRECEDING ) AS count_trips_window_30m_dropoff_zip,\n" +
                        "case when extract (ISODOW from  CAST (lpep_dropoff_datetime AS TIMESTAMP))  > 5 then 1 else 0 end as dropoff_is_weekend\n" +
                        "FROM green_tripdata";
        DBSPCompiler compiler = testCompiler();
        query = "CREATE VIEW V AS (" + query + ")";
        compiler.compileStatement(ddl);
        compiler.compileStatement(query);
        this.addRustTestCase("ComplexQueriesTest.taxiTest", compiler, getCircuit(compiler));
    }

    @Test
    public void fraudDetectionTest() {
        // fraudDetection-352718.cc_data.demo_
        String ddl0 = "CREATE TABLE demographics (\n" +
                " cc_num FLOAT64 NOT NULL,\n" +
                " first STRING,\n" +
                " gender STRING,\n" +
                " street STRING,\n" +
                " city STRING,\n" +
                " state STRING,\n" +
                " zip INTEGER,\n" +
                " lat FLOAT64,\n" +
                " long FLOAT64,\n" +
                " city_pop INTEGER,\n" +
                " job STRING,\n" +
                " dob DATE\n" +
                ")";
        String ddl1 = "CREATE TABLE transactions (\n" +
                " trans_date_trans_time TIMESTAMP NOT NULL,\n" +
                " cc_num FLOAT64,\n" +
                " merchant STRING,\n" +
                " category STRING,\n" +
                " amt FLOAT64,\n" +
                " trans_num STRING,\n" +
                " unix_time INTEGER NOT NULL,\n" +
                " merch_lat FLOAT64,\n" +
                " merch_long FLOAT64,\n" +
                " is_fraud INTEGER\n" +
                ")";
        String query = "SELECT\n" +
                "    DAYOFWEEK(trans_date_trans_time) AS d,\n" +
                "    TIMESTAMPDIFF(YEAR, trans_date_trans_time, CAST(dob as TIMESTAMP)) AS age,\n" +
                "    ST_DISTANCE(ST_POINT(long,lat), ST_POINT(merch_long,merch_lat)) AS distance,\n" +
                "    -- TIMESTAMPDIFF(MINUTE, trans_date_trans_time, last_txn_date) AS trans_diff,\n" +
                "    AVG(amt) OVER(\n" +
                "                PARTITION BY   CAST(cc_num AS NUMERIC)\n" +
                "                ORDER BY unix_time\n" +
                "                -- 1 week is 604800  seconds\n" +
                "                RANGE BETWEEN 604800  PRECEDING AND 1 PRECEDING) AS\n" +
                "avg_spend_pw,\n" +
                "      AVG(amt) OVER(\n" +
                "                PARTITION BY  CAST(cc_num AS NUMERIC)\n" +
                "                ORDER BY unix_time\n" +
                "                -- 1 month(30 days) is 2592000 seconds\n" +
                "                RANGE BETWEEN 2592000 PRECEDING AND 1 PRECEDING) AS\n" +
                "avg_spend_pm,\n" +
                "    COUNT(*) OVER(\n" +
                "                PARTITION BY  CAST(cc_num AS NUMERIC)\n" +
                "                ORDER BY unix_time\n" +
                "                -- 1 day is 86400  seconds\n" +
                "                RANGE BETWEEN 86400  PRECEDING AND 1 PRECEDING ) AS\n" +
                "trans_freq_24,\n" +
                "  category,\n" +
                "    amt,\n" +
                "    state,\n" +
                "    job,\n" +
                "    unix_time,\n" +
                "    city_pop,\n" +
                "    merchant,\n" +
                "    is_fraud\n" +
                "  FROM (\n" +
                "          SELECT t1.*, t2.*\n" +
                "          --,    LAG(trans_date_trans_time, 1) OVER (PARTITION BY t1.cc_num\n" +
                "          -- ORDER BY trans_date_trans_time ASC) AS last_txn_date\n" +
                "          FROM  transactions AS t1\n" +
                "          LEFT JOIN  demographics AS t2\n" +
                "          ON t1.cc_num =t2.cc_num)";
        DBSPCompiler compiler = testCompiler();
        query = "CREATE VIEW V AS (" + query + ")";
        compiler.compileStatement(ddl0);
        compiler.compileStatement(ddl1);
        compiler.compileStatement(query);
        this.addRustTestCase("ComplexQueriesTest.fraudDetectionTest", compiler, getCircuit(compiler));
    }
}
