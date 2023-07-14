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
import org.dbsp.sqlCompiler.ir.DBSPFunction;
import org.dbsp.util.Utilities;
import org.junit.AfterClass;
import org.junit.BeforeClass;

import java.io.IOException;
import java.io.PrintStream;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.List;

/**
 * Base class for SQL-based tests.
 */
public class BaseSQLTests {
    public static final String rustDirectory = "../temp/src";
    public static final String testFilePath = rustDirectory + "/lib.rs";

    /**
     * Collect here all the tests to run and execute them using a single Rust compilation.
     */
    static final List<TestCase> testsToRun = new ArrayList<>();

    @BeforeClass
    public static void prepareTests() {
        testsToRun.clear();
    }

    /**
     * Runs all the tests from the testsToRun list.
     */
    @AfterClass
    public static void runAllTests() throws IOException, InterruptedException {
        if (testsToRun.isEmpty())
            return;
        PrintStream outputStream = new PrintStream(Files.newOutputStream(Paths.get(testFilePath)));
        // Use the compiler from the first test case.
        DBSPCompiler firstCompiler = testsToRun.get(0).compiler;
        RustFileWriter writer = new RustFileWriter(firstCompiler, outputStream);
        int testNumber = 0;
        String[] extraArgs = new String[0];
        for (TestCase test: testsToRun) {
            if (!test.compiler.options.same(firstCompiler.options))
                throw new RuntimeException("Tests are not compiled with the same options: "
                        + test.compiler.options + " and " + firstCompiler.options);
            if (test.compiler.options.ioOptions.jit) {
                DBSPFunction tester = test.createJITTesterCode(testNumber);
                writer.add(tester);
                if (extraArgs.length == 0) {
                    extraArgs = new String[2];
                    extraArgs[0] = "--features";
                    extraArgs[1] = "jit";
                }
            } else {
                // Standard test
                writer.add(test.circuit);
                DBSPFunction tester = test.createTesterCode(testNumber);
                writer.add(tester);
            }
            testNumber++;
        }
        writer.writeAndClose();
        Utilities.compileAndTestRust(rustDirectory, true, extraArgs);
        testsToRun.clear();
    }

    protected void addRustTestCase(String name, DBSPCompiler compiler, DBSPCircuit circuit, InputOutputPair... streams) {
        TestCase test = new TestCase(name, compiler, circuit, streams);
        testsToRun.add(test);
    }

    /**
     * Create CompilerOptions according to the specified properties.
     * @param incremental  Generate an incremental program if true.
     * @param optimize     Optimize program if true.
     * @param jit          Generate code for the JIT if true.
     */
    public CompilerOptions testOptions(boolean incremental, boolean optimize, boolean jit) {
        CompilerOptions options = new CompilerOptions();
        options.optimizerOptions.throwOnError = true;
        options.optimizerOptions.generateInputForEveryTable = true;
        options.ioOptions.jit = jit;
        options.optimizerOptions.incrementalize = incremental;
        options.optimizerOptions.optimizationLevel = optimize ? 2 : 1;
        return options;
    }

    /**
     * Return the default compiler used for testing.
     */
    public DBSPCompiler testCompiler() {
        CompilerOptions options = this.testOptions(false, true, false);
        return new DBSPCompiler(options);
    }

    public static DBSPCircuit getCircuit(DBSPCompiler compiler) {
        compiler.optimize();
        String name = "circuit" + testsToRun.size();
        return compiler.getFinalCircuit(name);
    }
}
