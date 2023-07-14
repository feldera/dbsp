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

import org.dbsp.sqlCompiler.circuit.DBSPCircuit;
import org.dbsp.sqlCompiler.compiler.InputOutputPair;
import org.dbsp.sqlCompiler.compiler.backend.DBSPCompiler;
import org.dbsp.sqlCompiler.compiler.frontend.CalciteObject;
import org.dbsp.sqlCompiler.ir.expression.literal.DBSPZSetLiteral;
import org.dbsp.sqlCompiler.ir.type.DBSPTypeTuple;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeDecimal;
import org.dbsp.sqlCompiler.ir.type.primitive.DBSPTypeInteger;
import org.junit.Ignore;
import org.junit.Test;

/**
 * Tests manually adapted from
 * https://github.com/postgres/postgres/blob/master/src/test/regress/expected/numeric.out
 */
@SuppressWarnings("JavadocLinkAsPlainText")
public class PostgresNumericTests extends PostgresBaseTest {
    protected static final int WIDTH = 25;

    @Override
    public void prepareData(DBSPCompiler compiler) {
        String createTables = "CREATE TABLE num_data (id int4, val numeric(" + WIDTH + ",10));\n" +
                "CREATE TABLE num_exp_add (id1 int4, id2 int4, expected numeric(" + WIDTH + ",10));\n" +
                "CREATE TABLE num_exp_sub (id1 int4, id2 int4, expected numeric(" + WIDTH + ",10));\n" +
                "CREATE TABLE num_exp_div (id1 int4, id2 int4, expected numeric(" + WIDTH + ",10));\n" +
                "CREATE TABLE num_exp_mul (id1 int4, id2 int4, expected numeric(" + WIDTH + ",10));\n" +
                "CREATE TABLE num_exp_sqrt (id int4, expected numeric(" + WIDTH + ",10));\n" +
                "CREATE TABLE num_exp_ln (id int4, expected numeric(" + WIDTH + ",10));\n" +
                "CREATE TABLE num_exp_log10 (id int4, expected numeric(" + WIDTH + ",10));\n" +
                "CREATE TABLE num_exp_power_10_ln (id int4, expected numeric(" + WIDTH + ",10));\n";
        String insert =
                "INSERT INTO num_exp_add VALUES (0,0,'0');\n" +
                "INSERT INTO num_exp_sub VALUES (0,0,'0');\n" +
                "INSERT INTO num_exp_mul VALUES (0,0,'0');\n" +
                "INSERT INTO num_exp_div VALUES (0,0,'NaN');\n" +
                "INSERT INTO num_exp_add VALUES (0,1,'0');\n" +
                "INSERT INTO num_exp_sub VALUES (0,1,'0');\n" +
                "INSERT INTO num_exp_mul VALUES (0,1,'0');\n" +
                "INSERT INTO num_exp_div VALUES (0,1,'NaN');\n" +
                "INSERT INTO num_exp_add VALUES (0,2,'-34338492.215397047');\n" +
                "INSERT INTO num_exp_sub VALUES (0,2,'34338492.215397047');\n" +
                "INSERT INTO num_exp_mul VALUES (0,2,'0');\n" +
                "INSERT INTO num_exp_div VALUES (0,2,'0');\n" +
                "INSERT INTO num_exp_add VALUES (0,3,'4.31');\n" +
                "INSERT INTO num_exp_sub VALUES (0,3,'-4.31');\n" +
                "INSERT INTO num_exp_mul VALUES (0,3,'0');\n" +
                "INSERT INTO num_exp_div VALUES (0,3,'0');\n" +
                "INSERT INTO num_exp_add VALUES (0,4,'7799461.4119');\n" +
                "INSERT INTO num_exp_sub VALUES (0,4,'-7799461.4119');\n" +
                "INSERT INTO num_exp_mul VALUES (0,4,'0');\n" +
                "INSERT INTO num_exp_div VALUES (0,4,'0');\n" +
                "INSERT INTO num_exp_add VALUES (0,5,'16397.038491');\n" +
                "INSERT INTO num_exp_sub VALUES (0,5,'-16397.038491');\n" +
                "INSERT INTO num_exp_mul VALUES (0,5,'0');\n" +
                "INSERT INTO num_exp_div VALUES (0,5,'0');\n" +
                "INSERT INTO num_exp_add VALUES (0,6,'93901.57763026');\n" +
                "INSERT INTO num_exp_sub VALUES (0,6,'-93901.57763026');\n" +
                "INSERT INTO num_exp_mul VALUES (0,6,'0');\n" +
                "INSERT INTO num_exp_div VALUES (0,6,'0');\n" +
                "INSERT INTO num_exp_add VALUES (0,7,'-83028485');\n" +
                "INSERT INTO num_exp_sub VALUES (0,7,'83028485');\n" +
                "INSERT INTO num_exp_mul VALUES (0,7,'0');\n" +
                "INSERT INTO num_exp_div VALUES (0,7,'0');\n" +
                "INSERT INTO num_exp_add VALUES (0,8,'74881');\n" +
                "INSERT INTO num_exp_sub VALUES (0,8,'-74881');\n" +
                "INSERT INTO num_exp_mul VALUES (0,8,'0');\n" +
                "INSERT INTO num_exp_div VALUES (0,8,'0');\n" +
                "INSERT INTO num_exp_add VALUES (0,9,'-24926804.045047420');\n" +
                "INSERT INTO num_exp_sub VALUES (0,9,'24926804.045047420');\n" +
                "INSERT INTO num_exp_mul VALUES (0,9,'0');\n" +
                "INSERT INTO num_exp_div VALUES (0,9,'0');\n" +
                "INSERT INTO num_exp_add VALUES (1,0,'0');\n" +
                "INSERT INTO num_exp_sub VALUES (1,0,'0');\n" +
                "INSERT INTO num_exp_mul VALUES (1,0,'0');\n" +
                "INSERT INTO num_exp_div VALUES (1,0,NULL);\n" + // In postgres this is NaN
                "INSERT INTO num_exp_add VALUES (1,1,'0');\n" +
                "INSERT INTO num_exp_sub VALUES (1,1,'0');\n" +
                "INSERT INTO num_exp_mul VALUES (1,1,'0');\n" +
                "INSERT INTO num_exp_div VALUES (1,1,NULL);\n" + // In postgres this is NaN
                "INSERT INTO num_exp_add VALUES (1,2,'-34338492.215397047');\n" +
                "INSERT INTO num_exp_sub VALUES (1,2,'34338492.215397047');\n" +
                "INSERT INTO num_exp_mul VALUES (1,2,'0');\n" +
                "INSERT INTO num_exp_div VALUES (1,2,'0');\n" +
                "INSERT INTO num_exp_add VALUES (1,3,'4.31');\n" +
                "INSERT INTO num_exp_sub VALUES (1,3,'-4.31');\n" +
                "INSERT INTO num_exp_mul VALUES (1,3,'0');\n" +
                "INSERT INTO num_exp_div VALUES (1,3,'0');\n" +
                "INSERT INTO num_exp_add VALUES (1,4,'7799461.4119');\n" +
                "INSERT INTO num_exp_sub VALUES (1,4,'-7799461.4119');\n" +
                "INSERT INTO num_exp_mul VALUES (1,4,'0');\n" +
                "INSERT INTO num_exp_div VALUES (1,4,'0');\n" +
                "INSERT INTO num_exp_add VALUES (1,5,'16397.038491');\n" +
                "INSERT INTO num_exp_sub VALUES (1,5,'-16397.038491');\n" +
                "INSERT INTO num_exp_mul VALUES (1,5,'0');\n" +
                "INSERT INTO num_exp_div VALUES (1,5,'0');\n" +
                "INSERT INTO num_exp_add VALUES (1,6,'93901.57763026');\n" +
                "INSERT INTO num_exp_sub VALUES (1,6,'-93901.57763026');\n" +
                "INSERT INTO num_exp_mul VALUES (1,6,'0');\n" +
                "INSERT INTO num_exp_div VALUES (1,6,'0');\n" +
                "INSERT INTO num_exp_add VALUES (1,7,'-83028485');\n" +
                "INSERT INTO num_exp_sub VALUES (1,7,'83028485');\n" +
                "INSERT INTO num_exp_mul VALUES (1,7,'0');\n" +
                "INSERT INTO num_exp_div VALUES (1,7,'0');\n" +
                "INSERT INTO num_exp_add VALUES (1,8,'74881');\n" +
                "INSERT INTO num_exp_sub VALUES (1,8,'-74881');\n" +
                "INSERT INTO num_exp_mul VALUES (1,8,'0');\n" +
                "INSERT INTO num_exp_div VALUES (1,8,'0');\n" +
                "INSERT INTO num_exp_add VALUES (1,9,'-24926804.045047420');\n" +
                "INSERT INTO num_exp_sub VALUES (1,9,'24926804.045047420');\n" +
                "INSERT INTO num_exp_mul VALUES (1,9,'0');\n" +
                "INSERT INTO num_exp_div VALUES (1,9,'0');\n" +
                "INSERT INTO num_exp_add VALUES (2,0,'-34338492.215397047');\n" +
                "INSERT INTO num_exp_sub VALUES (2,0,'-34338492.215397047');\n" +
                "INSERT INTO num_exp_mul VALUES (2,0,'0');\n" +
                "INSERT INTO num_exp_div VALUES (2,0,'NaN');\n" +
                "INSERT INTO num_exp_add VALUES (2,1,'-34338492.215397047');\n" +
                "INSERT INTO num_exp_sub VALUES (2,1,'-34338492.215397047');\n" +
                "INSERT INTO num_exp_mul VALUES (2,1,'0');\n" +
                "INSERT INTO num_exp_div VALUES (2,1,'NaN');\n" +
                "INSERT INTO num_exp_add VALUES (2,2,'-68676984.430794094');\n" +
                "INSERT INTO num_exp_sub VALUES (2,2,'0');\n" +
                "INSERT INTO num_exp_mul VALUES (2,2,'1179132047626883.596862135856320209');\n" +
                "INSERT INTO num_exp_div VALUES (2,2,'1.00000000000000000000');\n" +
                "INSERT INTO num_exp_add VALUES (2,3,'-34338487.905397047');\n" +
                "INSERT INTO num_exp_sub VALUES (2,3,'-34338496.525397047');\n" +
                "INSERT INTO num_exp_mul VALUES (2,3,'-147998901.44836127257');\n" +
                "INSERT INTO num_exp_div VALUES (2,3,'-7967167.56737750510440835266');\n" +
                "INSERT INTO num_exp_add VALUES (2,4,'-26539030.803497047');\n" +
                "INSERT INTO num_exp_sub VALUES (2,4,'-42137953.627297047');\n" +
                "INSERT INTO num_exp_mul VALUES (2,4,'-267821744976817.8111137106593');\n" +
                "INSERT INTO num_exp_div VALUES (2,4,'-4.40267480046830116685');\n" +
                "INSERT INTO num_exp_add VALUES (2,5,'-34322095.176906047');\n" +
                "INSERT INTO num_exp_sub VALUES (2,5,'-34354889.253888047');\n" +
                "INSERT INTO num_exp_mul VALUES (2,5,'-563049578578.769242506736077');\n" +
                "INSERT INTO num_exp_div VALUES (2,5,'-2094.18866914563535496429');\n" +
                "INSERT INTO num_exp_add VALUES (2,6,'-34244590.637766787');\n" +
                "INSERT INTO num_exp_sub VALUES (2,6,'-34432393.793027307');\n" +
                "INSERT INTO num_exp_mul VALUES (2,6,'-3224438592470.18449811926184222');\n" +
                "INSERT INTO num_exp_div VALUES (2,6,'-365.68599891479766440940');\n" +
                "INSERT INTO num_exp_add VALUES (2,7,'-117366977.215397047');\n" +
                "INSERT INTO num_exp_sub VALUES (2,7,'48689992.784602953');\n" +
                "INSERT INTO num_exp_mul VALUES (2,7,'2851072985828710.485883795');\n" +
                "INSERT INTO num_exp_div VALUES (2,7,'.41357483778485235518');\n" +
                "INSERT INTO num_exp_add VALUES (2,8,'-34263611.215397047');\n" +
                "INSERT INTO num_exp_sub VALUES (2,8,'-34413373.215397047');\n" +
                "INSERT INTO num_exp_mul VALUES (2,8,'-2571300635581.146276407');\n" +
                "INSERT INTO num_exp_div VALUES (2,8,'-458.57416721727870888476');\n" +
                "INSERT INTO num_exp_add VALUES (2,9,'-59265296.260444467');\n" +
                "INSERT INTO num_exp_sub VALUES (2,9,'-9411688.170349627');\n" +
                "INSERT INTO num_exp_mul VALUES (2,9,'855948866655588.453741509242968740');\n" +
                "INSERT INTO num_exp_div VALUES (2,9,'1.37757299946438931811');\n" +
                "INSERT INTO num_exp_add VALUES (3,0,'4.31');\n" +
                "INSERT INTO num_exp_sub VALUES (3,0,'4.31');\n" +
                "INSERT INTO num_exp_mul VALUES (3,0,'0');\n" +
                "INSERT INTO num_exp_div VALUES (3,0,'NaN');\n" +
                "INSERT INTO num_exp_add VALUES (3,1,'4.31');\n" +
                "INSERT INTO num_exp_sub VALUES (3,1,'4.31');\n" +
                "INSERT INTO num_exp_mul VALUES (3,1,'0');\n" +
                "INSERT INTO num_exp_div VALUES (3,1,'NaN');\n" +
                "INSERT INTO num_exp_add VALUES (3,2,'-34338487.905397047');\n" +
                "INSERT INTO num_exp_sub VALUES (3,2,'34338496.525397047');\n" +
                "INSERT INTO num_exp_mul VALUES (3,2,'-147998901.44836127257');\n" +
                "INSERT INTO num_exp_div VALUES (3,2,'-.00000012551512084352');\n" +
                "INSERT INTO num_exp_add VALUES (3,3,'8.62');\n" +
                "INSERT INTO num_exp_sub VALUES (3,3,'0');\n" +
                "INSERT INTO num_exp_mul VALUES (3,3,'18.5761');\n" +
                "INSERT INTO num_exp_div VALUES (3,3,'1.00000000000000000000');\n" +
                "INSERT INTO num_exp_add VALUES (3,4,'7799465.7219');\n" +
                "INSERT INTO num_exp_sub VALUES (3,4,'-7799457.1019');\n" +
                "INSERT INTO num_exp_mul VALUES (3,4,'33615678.685289');\n" +
                "INSERT INTO num_exp_div VALUES (3,4,'.00000055260225961552');\n" +
                "INSERT INTO num_exp_add VALUES (3,5,'16401.348491');\n" +
                "INSERT INTO num_exp_sub VALUES (3,5,'-16392.728491');\n" +
                "INSERT INTO num_exp_mul VALUES (3,5,'70671.23589621');\n" +
                "INSERT INTO num_exp_div VALUES (3,5,'.00026285234387695504');\n" +
                "INSERT INTO num_exp_add VALUES (3,6,'93905.88763026');\n" +
                "INSERT INTO num_exp_sub VALUES (3,6,'-93897.26763026');\n" +
                "INSERT INTO num_exp_mul VALUES (3,6,'404715.7995864206');\n" +
                "INSERT INTO num_exp_div VALUES (3,6,'.00004589912234457595');\n" +
                "INSERT INTO num_exp_add VALUES (3,7,'-83028480.69');\n" +
                "INSERT INTO num_exp_sub VALUES (3,7,'83028489.31');\n" +
                "INSERT INTO num_exp_mul VALUES (3,7,'-357852770.35');\n" +
                "INSERT INTO num_exp_div VALUES (3,7,'-.00000005190989574240');\n" +
                "INSERT INTO num_exp_add VALUES (3,8,'74885.31');\n" +
                "INSERT INTO num_exp_sub VALUES (3,8,'-74876.69');\n" +
                "INSERT INTO num_exp_mul VALUES (3,8,'322737.11');\n" +
                "INSERT INTO num_exp_div VALUES (3,8,'.00005755799201399553');\n" +
                "INSERT INTO num_exp_add VALUES (3,9,'-24926799.735047420');\n" +
                "INSERT INTO num_exp_sub VALUES (3,9,'24926808.355047420');\n" +
                "INSERT INTO num_exp_mul VALUES (3,9,'-107434525.43415438020');\n" +
                "INSERT INTO num_exp_div VALUES (3,9,'-.00000017290624149854');\n" +
                "INSERT INTO num_exp_add VALUES (4,0,'7799461.4119');\n" +
                "INSERT INTO num_exp_sub VALUES (4,0,'7799461.4119');\n" +
                "INSERT INTO num_exp_mul VALUES (4,0,'0');\n" +
                "INSERT INTO num_exp_div VALUES (4,0,'NaN');\n" +
                "INSERT INTO num_exp_add VALUES (4,1,'7799461.4119');\n" +
                "INSERT INTO num_exp_sub VALUES (4,1,'7799461.4119');\n" +
                "INSERT INTO num_exp_mul VALUES (4,1,'0');\n" +
                "INSERT INTO num_exp_div VALUES (4,1,'NaN');\n" +
                "INSERT INTO num_exp_add VALUES (4,2,'-26539030.803497047');\n" +
                "INSERT INTO num_exp_sub VALUES (4,2,'42137953.627297047');\n" +
                "INSERT INTO num_exp_mul VALUES (4,2,'-267821744976817.8111137106593');\n" +
                "INSERT INTO num_exp_div VALUES (4,2,'-.22713465002993920385');\n" +
                "INSERT INTO num_exp_add VALUES (4,3,'7799465.7219');\n" +
                "INSERT INTO num_exp_sub VALUES (4,3,'7799457.1019');\n" +
                "INSERT INTO num_exp_mul VALUES (4,3,'33615678.685289');\n" +
                "INSERT INTO num_exp_div VALUES (4,3,'1809619.81714617169373549883');\n" +
                "INSERT INTO num_exp_add VALUES (4,4,'15598922.8238');\n" +
                "INSERT INTO num_exp_sub VALUES (4,4,'0');\n" +
                "INSERT INTO num_exp_mul VALUES (4,4,'60831598315717.14146161');\n" +
                "INSERT INTO num_exp_div VALUES (4,4,'1.00000000000000000000');\n" +
                "INSERT INTO num_exp_add VALUES (4,5,'7815858.450391');\n" +
                "INSERT INTO num_exp_sub VALUES (4,5,'7783064.373409');\n" +
                "INSERT INTO num_exp_mul VALUES (4,5,'127888068979.9935054429');\n" +
                "INSERT INTO num_exp_div VALUES (4,5,'475.66281046305802686061');\n" +
                "INSERT INTO num_exp_add VALUES (4,6,'7893362.98953026');\n" +
                "INSERT INTO num_exp_sub VALUES (4,6,'7705559.83426974');\n" +
                "INSERT INTO num_exp_mul VALUES (4,6,'732381731243.745115764094');\n" +
                "INSERT INTO num_exp_div VALUES (4,6,'83.05996138436129499606');\n" +
                "INSERT INTO num_exp_add VALUES (4,7,'-75229023.5881');\n" +
                "INSERT INTO num_exp_sub VALUES (4,7,'90827946.4119');\n" +
                "INSERT INTO num_exp_mul VALUES (4,7,'-647577464846017.9715');\n" +
                "INSERT INTO num_exp_div VALUES (4,7,'-.09393717604145131637');\n" +
                "INSERT INTO num_exp_add VALUES (4,8,'7874342.4119');\n" +
                "INSERT INTO num_exp_sub VALUES (4,8,'7724580.4119');\n" +
                "INSERT INTO num_exp_mul VALUES (4,8,'584031469984.4839');\n" +
                "INSERT INTO num_exp_div VALUES (4,8,'104.15808298366741897143');\n" +
                "INSERT INTO num_exp_add VALUES (4,9,'-17127342.633147420');\n" +
                "INSERT INTO num_exp_sub VALUES (4,9,'32726265.456947420');\n" +
                "INSERT INTO num_exp_mul VALUES (4,9,'-194415646271340.1815956522980');\n" +
                "INSERT INTO num_exp_div VALUES (4,9,'-.31289456112403769409');\n" +
                "INSERT INTO num_exp_add VALUES (5,0,'16397.038491');\n" +
                "INSERT INTO num_exp_sub VALUES (5,0,'16397.038491');\n" +
                "INSERT INTO num_exp_mul VALUES (5,0,'0');\n" +
                "INSERT INTO num_exp_div VALUES (5,0,'NaN');\n" +
                "INSERT INTO num_exp_add VALUES (5,1,'16397.038491');\n" +
                "INSERT INTO num_exp_sub VALUES (5,1,'16397.038491');\n" +
                "INSERT INTO num_exp_mul VALUES (5,1,'0');\n" +
                "INSERT INTO num_exp_div VALUES (5,1,'NaN');\n" +
                "INSERT INTO num_exp_add VALUES (5,2,'-34322095.176906047');\n" +
                "INSERT INTO num_exp_sub VALUES (5,2,'34354889.253888047');\n" +
                "INSERT INTO num_exp_mul VALUES (5,2,'-563049578578.769242506736077');\n" +
                "INSERT INTO num_exp_div VALUES (5,2,'-.00047751189505192446');\n" +
                "INSERT INTO num_exp_add VALUES (5,3,'16401.348491');\n" +
                "INSERT INTO num_exp_sub VALUES (5,3,'16392.728491');\n" +
                "INSERT INTO num_exp_mul VALUES (5,3,'70671.23589621');\n" +
                "INSERT INTO num_exp_div VALUES (5,3,'3804.41728329466357308584');\n" +
                "INSERT INTO num_exp_add VALUES (5,4,'7815858.450391');\n" +
                "INSERT INTO num_exp_sub VALUES (5,4,'-7783064.373409');\n" +
                "INSERT INTO num_exp_mul VALUES (5,4,'127888068979.9935054429');\n" +
                "INSERT INTO num_exp_div VALUES (5,4,'.00210232958726897192');\n" +
                "INSERT INTO num_exp_add VALUES (5,5,'32794.076982');\n" +
                "INSERT INTO num_exp_sub VALUES (5,5,'0');\n" +
                "INSERT INTO num_exp_mul VALUES (5,5,'268862871.275335557081');\n" +
                "INSERT INTO num_exp_div VALUES (5,5,'1.00000000000000000000');\n" +
                "INSERT INTO num_exp_add VALUES (5,6,'110298.61612126');\n" +
                "INSERT INTO num_exp_sub VALUES (5,6,'-77504.53913926');\n" +
                "INSERT INTO num_exp_mul VALUES (5,6,'1539707782.76899778633766');\n" +
                "INSERT INTO num_exp_div VALUES (5,6,'.17461941433576102689');\n" +
                "INSERT INTO num_exp_add VALUES (5,7,'-83012087.961509');\n" +
                "INSERT INTO num_exp_sub VALUES (5,7,'83044882.038491');\n" +
                "INSERT INTO num_exp_mul VALUES (5,7,'-1361421264394.416135');\n" +
                "INSERT INTO num_exp_div VALUES (5,7,'-.00019748690453643710');\n" +
                "INSERT INTO num_exp_add VALUES (5,8,'91278.038491');\n" +
                "INSERT INTO num_exp_sub VALUES (5,8,'-58483.961509');\n" +
                "INSERT INTO num_exp_mul VALUES (5,8,'1227826639.244571');\n" +
                "INSERT INTO num_exp_div VALUES (5,8,'.21897461960978085228');\n" +
                "INSERT INTO num_exp_add VALUES (5,9,'-24910407.006556420');\n" +
                "INSERT INTO num_exp_sub VALUES (5,9,'24943201.083538420');\n" +
                "INSERT INTO num_exp_mul VALUES (5,9,'-408725765384.257043660243220');\n" +
                "INSERT INTO num_exp_div VALUES (5,9,'-.00065780749354660427');\n" +
                "INSERT INTO num_exp_add VALUES (6,0,'93901.57763026');\n" +
                "INSERT INTO num_exp_sub VALUES (6,0,'93901.57763026');\n" +
                "INSERT INTO num_exp_mul VALUES (6,0,'0');\n" +
                "INSERT INTO num_exp_div VALUES (6,0,'NaN');\n" +
                "INSERT INTO num_exp_add VALUES (6,1,'93901.57763026');\n" +
                "INSERT INTO num_exp_sub VALUES (6,1,'93901.57763026');\n" +
                "INSERT INTO num_exp_mul VALUES (6,1,'0');\n" +
                "INSERT INTO num_exp_div VALUES (6,1,'NaN');\n" +
                "INSERT INTO num_exp_add VALUES (6,2,'-34244590.637766787');\n" +
                "INSERT INTO num_exp_sub VALUES (6,2,'34432393.793027307');\n" +
                "INSERT INTO num_exp_mul VALUES (6,2,'-3224438592470.18449811926184222');\n" +
                "INSERT INTO num_exp_div VALUES (6,2,'-.00273458651128995823');\n" +
                "INSERT INTO num_exp_add VALUES (6,3,'93905.88763026');\n" +
                "INSERT INTO num_exp_sub VALUES (6,3,'93897.26763026');\n" +
                "INSERT INTO num_exp_mul VALUES (6,3,'404715.7995864206');\n" +
                "INSERT INTO num_exp_div VALUES (6,3,'21786.90896293735498839907');\n" +
                "INSERT INTO num_exp_add VALUES (6,4,'7893362.98953026');\n" +
                "INSERT INTO num_exp_sub VALUES (6,4,'-7705559.83426974');\n" +
                "INSERT INTO num_exp_mul VALUES (6,4,'732381731243.745115764094');\n" +
                "INSERT INTO num_exp_div VALUES (6,4,'.01203949512295682469');\n" +
                "INSERT INTO num_exp_add VALUES (6,5,'110298.61612126');\n" +
                "INSERT INTO num_exp_sub VALUES (6,5,'77504.53913926');\n" +
                "INSERT INTO num_exp_mul VALUES (6,5,'1539707782.76899778633766');\n" +
                "INSERT INTO num_exp_div VALUES (6,5,'5.72674008674192359679');\n" +
                "INSERT INTO num_exp_add VALUES (6,6,'187803.15526052');\n" +
                "INSERT INTO num_exp_sub VALUES (6,6,'0');\n" +
                "INSERT INTO num_exp_mul VALUES (6,6,'8817506281.4517452372676676');\n" +
                "INSERT INTO num_exp_div VALUES (6,6,'1.00000000000000000000');\n" +
                "INSERT INTO num_exp_add VALUES (6,7,'-82934583.42236974');\n" +
                "INSERT INTO num_exp_sub VALUES (6,7,'83122386.57763026');\n" +
                "INSERT INTO num_exp_mul VALUES (6,7,'-7796505729750.37795610');\n" +
                "INSERT INTO num_exp_div VALUES (6,7,'-.00113095617281538980');\n" +
                "INSERT INTO num_exp_add VALUES (6,8,'168782.57763026');\n" +
                "INSERT INTO num_exp_sub VALUES (6,8,'19020.57763026');\n" +
                "INSERT INTO num_exp_mul VALUES (6,8,'7031444034.53149906');\n" +
                "INSERT INTO num_exp_div VALUES (6,8,'1.25401073209839612184');\n" +
                "INSERT INTO num_exp_add VALUES (6,9,'-24832902.467417160');\n" +
                "INSERT INTO num_exp_sub VALUES (6,9,'25020705.622677680');\n" +
                "INSERT INTO num_exp_mul VALUES (6,9,'-2340666225110.29929521292692920');\n" +
                "INSERT INTO num_exp_div VALUES (6,9,'-.00376709254265256789');\n" +
                "INSERT INTO num_exp_add VALUES (7,0,'-83028485');\n" +
                "INSERT INTO num_exp_sub VALUES (7,0,'-83028485');\n" +
                "INSERT INTO num_exp_mul VALUES (7,0,'0');\n" +
                "INSERT INTO num_exp_div VALUES (7,0,'NaN');\n" +
                "INSERT INTO num_exp_add VALUES (7,1,'-83028485');\n" +
                "INSERT INTO num_exp_sub VALUES (7,1,'-83028485');\n" +
                "INSERT INTO num_exp_mul VALUES (7,1,'0');\n" +
                "INSERT INTO num_exp_div VALUES (7,1,'NaN');\n" +
                "INSERT INTO num_exp_add VALUES (7,2,'-117366977.215397047');\n" +
                "INSERT INTO num_exp_sub VALUES (7,2,'-48689992.784602953');\n" +
                "INSERT INTO num_exp_mul VALUES (7,2,'2851072985828710.485883795');\n" +
                "INSERT INTO num_exp_div VALUES (7,2,'2.41794207151503385700');\n" +
                "INSERT INTO num_exp_add VALUES (7,3,'-83028480.69');\n" +
                "INSERT INTO num_exp_sub VALUES (7,3,'-83028489.31');\n" +
                "INSERT INTO num_exp_mul VALUES (7,3,'-357852770.35');\n" +
                "INSERT INTO num_exp_div VALUES (7,3,'-19264149.65197215777262180974');\n" +
                "INSERT INTO num_exp_add VALUES (7,4,'-75229023.5881');\n" +
                "INSERT INTO num_exp_sub VALUES (7,4,'-90827946.4119');\n" +
                "INSERT INTO num_exp_mul VALUES (7,4,'-647577464846017.9715');\n" +
                "INSERT INTO num_exp_div VALUES (7,4,'-10.64541262725136247686');\n" +
                "INSERT INTO num_exp_add VALUES (7,5,'-83012087.961509');\n" +
                "INSERT INTO num_exp_sub VALUES (7,5,'-83044882.038491');\n" +
                "INSERT INTO num_exp_mul VALUES (7,5,'-1361421264394.416135');\n" +
                "INSERT INTO num_exp_div VALUES (7,5,'-5063.62688881730941836574');\n" +
                "INSERT INTO num_exp_add VALUES (7,6,'-82934583.42236974');\n" +
                "INSERT INTO num_exp_sub VALUES (7,6,'-83122386.57763026');\n" +
                "INSERT INTO num_exp_mul VALUES (7,6,'-7796505729750.37795610');\n" +
                "INSERT INTO num_exp_div VALUES (7,6,'-884.20756174009028770294');\n" +
                "INSERT INTO num_exp_add VALUES (7,7,'-166056970');\n" +
                "INSERT INTO num_exp_sub VALUES (7,7,'0');\n" +
                "INSERT INTO num_exp_mul VALUES (7,7,'6893729321395225');\n" +
                "INSERT INTO num_exp_div VALUES (7,7,'1.00000000000000000000');\n" +
                "INSERT INTO num_exp_add VALUES (7,8,'-82953604');\n" +
                "INSERT INTO num_exp_sub VALUES (7,8,'-83103366');\n" +
                "INSERT INTO num_exp_mul VALUES (7,8,'-6217255985285');\n" +
                "INSERT INTO num_exp_div VALUES (7,8,'-1108.80577182462841041118');\n" +
                "INSERT INTO num_exp_add VALUES (7,9,'-107955289.045047420');\n" +
                "INSERT INTO num_exp_sub VALUES (7,9,'-58101680.954952580');\n" +
                "INSERT INTO num_exp_mul VALUES (7,9,'2069634775752159.035758700');\n" +
                "INSERT INTO num_exp_div VALUES (7,9,'3.33089171198810413382');\n" +
                "INSERT INTO num_exp_add VALUES (8,0,'74881');\n" +
                "INSERT INTO num_exp_sub VALUES (8,0,'74881');\n" +
                "INSERT INTO num_exp_mul VALUES (8,0,'0');\n" +
                "INSERT INTO num_exp_div VALUES (8,0,'NaN');\n" +
                "INSERT INTO num_exp_add VALUES (8,1,'74881');\n" +
                "INSERT INTO num_exp_sub VALUES (8,1,'74881');\n" +
                "INSERT INTO num_exp_mul VALUES (8,1,'0');\n" +
                "INSERT INTO num_exp_div VALUES (8,1,'NaN');\n" +
                "INSERT INTO num_exp_add VALUES (8,2,'-34263611.215397047');\n" +
                "INSERT INTO num_exp_sub VALUES (8,2,'34413373.215397047');\n" +
                "INSERT INTO num_exp_mul VALUES (8,2,'-2571300635581.146276407');\n" +
                "INSERT INTO num_exp_div VALUES (8,2,'-.00218067233500788615');\n" +
                "INSERT INTO num_exp_add VALUES (8,3,'74885.31');\n" +
                "INSERT INTO num_exp_sub VALUES (8,3,'74876.69');\n" +
                "INSERT INTO num_exp_mul VALUES (8,3,'322737.11');\n" +
                "INSERT INTO num_exp_div VALUES (8,3,'17373.78190255220417633410');\n" +
                "INSERT INTO num_exp_add VALUES (8,4,'7874342.4119');\n" +
                "INSERT INTO num_exp_sub VALUES (8,4,'-7724580.4119');\n" +
                "INSERT INTO num_exp_mul VALUES (8,4,'584031469984.4839');\n" +
                "INSERT INTO num_exp_div VALUES (8,4,'.00960079113741758956');\n" +
                "INSERT INTO num_exp_add VALUES (8,5,'91278.038491');\n" +
                "INSERT INTO num_exp_sub VALUES (8,5,'58483.961509');\n" +
                "INSERT INTO num_exp_mul VALUES (8,5,'1227826639.244571');\n" +
                "INSERT INTO num_exp_div VALUES (8,5,'4.56673929509287019456');\n" +
                "INSERT INTO num_exp_add VALUES (8,6,'168782.57763026');\n" +
                "INSERT INTO num_exp_sub VALUES (8,6,'-19020.57763026');\n" +
                "INSERT INTO num_exp_mul VALUES (8,6,'7031444034.53149906');\n" +
                "INSERT INTO num_exp_div VALUES (8,6,'.79744134113322314424');\n" +
                "INSERT INTO num_exp_add VALUES (8,7,'-82953604');\n" +
                "INSERT INTO num_exp_sub VALUES (8,7,'83103366');\n" +
                "INSERT INTO num_exp_mul VALUES (8,7,'-6217255985285');\n" +
                "INSERT INTO num_exp_div VALUES (8,7,'-.00090187120721280172');\n" +
                "INSERT INTO num_exp_add VALUES (8,8,'149762');\n" +
                "INSERT INTO num_exp_sub VALUES (8,8,'0');\n" +
                "INSERT INTO num_exp_mul VALUES (8,8,'5607164161');\n" +
                "INSERT INTO num_exp_div VALUES (8,8,'1.00000000000000000000');\n" +
                "INSERT INTO num_exp_add VALUES (8,9,'-24851923.045047420');\n" +
                "INSERT INTO num_exp_sub VALUES (8,9,'25001685.045047420');\n" +
                "INSERT INTO num_exp_mul VALUES (8,9,'-1866544013697.195857020');\n" +
                "INSERT INTO num_exp_div VALUES (8,9,'-.00300403532938582735');\n" +
                "INSERT INTO num_exp_add VALUES (9,0,'-24926804.045047420');\n" +
                "INSERT INTO num_exp_sub VALUES (9,0,'-24926804.045047420');\n" +
                "INSERT INTO num_exp_mul VALUES (9,0,'0');\n" +
                "INSERT INTO num_exp_div VALUES (9,0,'NaN');\n" +
                "INSERT INTO num_exp_add VALUES (9,1,'-24926804.045047420');\n" +
                "INSERT INTO num_exp_sub VALUES (9,1,'-24926804.045047420');\n" +
                "INSERT INTO num_exp_mul VALUES (9,1,'0');\n" +
                "INSERT INTO num_exp_div VALUES (9,1,'NaN');\n" +
                "INSERT INTO num_exp_add VALUES (9,2,'-59265296.260444467');\n" +
                "INSERT INTO num_exp_sub VALUES (9,2,'9411688.170349627');\n" +
                "INSERT INTO num_exp_mul VALUES (9,2,'855948866655588.453741509242968740');\n" +
                "INSERT INTO num_exp_div VALUES (9,2,'.72591434384152961526');\n" +
                "INSERT INTO num_exp_add VALUES (9,3,'-24926799.735047420');\n" +
                "INSERT INTO num_exp_sub VALUES (9,3,'-24926808.355047420');\n" +
                "INSERT INTO num_exp_mul VALUES (9,3,'-107434525.43415438020');\n" +
                "INSERT INTO num_exp_div VALUES (9,3,'-5783481.21694835730858468677');\n" +
                "INSERT INTO num_exp_add VALUES (9,4,'-17127342.633147420');\n" +
                "INSERT INTO num_exp_sub VALUES (9,4,'-32726265.456947420');\n" +
                "INSERT INTO num_exp_mul VALUES (9,4,'-194415646271340.1815956522980');\n" +
                "INSERT INTO num_exp_div VALUES (9,4,'-3.19596478892958416484');\n" +
                "INSERT INTO num_exp_add VALUES (9,5,'-24910407.006556420');\n" +
                "INSERT INTO num_exp_sub VALUES (9,5,'-24943201.083538420');\n" +
                "INSERT INTO num_exp_mul VALUES (9,5,'-408725765384.257043660243220');\n" +
                "INSERT INTO num_exp_div VALUES (9,5,'-1520.20159364322004505807');\n" +
                "INSERT INTO num_exp_add VALUES (9,6,'-24832902.467417160');\n" +
                "INSERT INTO num_exp_sub VALUES (9,6,'-25020705.622677680');\n" +
                "INSERT INTO num_exp_mul VALUES (9,6,'-2340666225110.29929521292692920');\n" +
                "INSERT INTO num_exp_div VALUES (9,6,'-265.45671195426965751280');\n" +
                "INSERT INTO num_exp_add VALUES (9,7,'-107955289.045047420');\n" +
                "INSERT INTO num_exp_sub VALUES (9,7,'58101680.954952580');\n" +
                "INSERT INTO num_exp_mul VALUES (9,7,'2069634775752159.035758700');\n" +
                "INSERT INTO num_exp_div VALUES (9,7,'.30021990699995814689');\n" +
                "INSERT INTO num_exp_add VALUES (9,8,'-24851923.045047420');\n" +
                "INSERT INTO num_exp_sub VALUES (9,8,'-25001685.045047420');\n" +
                "INSERT INTO num_exp_mul VALUES (9,8,'-1866544013697.195857020');\n" +
                "INSERT INTO num_exp_div VALUES (9,8,'-332.88556569820675471748');\n" +
                "INSERT INTO num_exp_add VALUES (9,9,'-49853608.090094840');\n" +
                "INSERT INTO num_exp_sub VALUES (9,9,'0');\n" +
                "INSERT INTO num_exp_mul VALUES (9,9,'621345559900192.420120630048656400');\n" +
                "INSERT INTO num_exp_div VALUES (9,9,'1.00000000000000000000');\n" +
                "INSERT INTO num_exp_sqrt VALUES (0,'0');\n" +
                "INSERT INTO num_exp_sqrt VALUES (1,'0');\n" +
                "INSERT INTO num_exp_sqrt VALUES (2,'5859.90547836712524903505');\n" +
                "INSERT INTO num_exp_sqrt VALUES (3,'2.07605394920266944396');\n" +
                "INSERT INTO num_exp_sqrt VALUES (4,'2792.75158435189147418923');\n" +
                "INSERT INTO num_exp_sqrt VALUES (5,'128.05092147657509145473');\n" +
                "INSERT INTO num_exp_sqrt VALUES (6,'306.43364311096782703406');\n" +
                "INSERT INTO num_exp_sqrt VALUES (7,'9111.99676251039939975230');\n" +
                "INSERT INTO num_exp_sqrt VALUES (8,'273.64392922189960397542');\n" +
                "INSERT INTO num_exp_sqrt VALUES (9,'4992.67503899937593364766');\n" +
                "INSERT INTO num_exp_ln VALUES (0,'NaN');\n" +
                "INSERT INTO num_exp_ln VALUES (1,'NaN');\n" +
                "INSERT INTO num_exp_ln VALUES (2,'17.35177750493897715514');\n" +
                "INSERT INTO num_exp_ln VALUES (3,'1.46093790411565641971');\n" +
                "INSERT INTO num_exp_ln VALUES (4,'15.86956523951936572464');\n" +
                "INSERT INTO num_exp_ln VALUES (5,'9.70485601768871834038');\n" +
                "INSERT INTO num_exp_ln VALUES (6,'11.45000246622944403127');\n" +
                "INSERT INTO num_exp_ln VALUES (7,'18.23469429965478772991');\n" +
                "INSERT INTO num_exp_ln VALUES (8,'11.22365546576315513668');\n" +
                "INSERT INTO num_exp_ln VALUES (9,'17.03145425013166006962');\n" +
                "INSERT INTO num_exp_log10 VALUES (0,'NaN');\n" +
                "INSERT INTO num_exp_log10 VALUES (1,'NaN');\n" +
                "INSERT INTO num_exp_log10 VALUES (2,'7.53578122160797276459');\n" +
                "INSERT INTO num_exp_log10 VALUES (3,'.63447727016073160075');\n" +
                "INSERT INTO num_exp_log10 VALUES (4,'6.89206461372691743345');\n" +
                "INSERT INTO num_exp_log10 VALUES (5,'4.21476541614777768626');\n" +
                "INSERT INTO num_exp_log10 VALUES (6,'4.97267288886207207671');\n" +
                "INSERT INTO num_exp_log10 VALUES (7,'7.91922711353275546914');\n" +
                "INSERT INTO num_exp_log10 VALUES (8,'4.87437163556421004138');\n" +
                "INSERT INTO num_exp_log10 VALUES (9,'7.39666659961986567059');\n" +
                "INSERT INTO num_exp_power_10_ln VALUES (0,'NaN');\n" +
                "INSERT INTO num_exp_power_10_ln VALUES (1,'NaN');\n" +
                "INSERT INTO num_exp_power_10_ln VALUES (2,'224790267919917955.13261618583642653184');\n" +
                "INSERT INTO num_exp_power_10_ln VALUES (3,'28.90266599445155957393');\n" +
                "INSERT INTO num_exp_power_10_ln VALUES (4,'7405685069594999.07733999469386277636');\n" +
                "INSERT INTO num_exp_power_10_ln VALUES (5,'5068226527.32127265408584640098');\n" +
                "INSERT INTO num_exp_power_10_ln VALUES (6,'281839893606.99372343357047819067');\n" +
                "INSERT INTO num_exp_power_10_ln VALUES (7,'1716699575118597095.42330819910640247627');\n" +
                "INSERT INTO num_exp_power_10_ln VALUES (8,'167361463828.07491320069016125952');\n" +
                "INSERT INTO num_exp_power_10_ln VALUES (9,'107511333880052007.04141124673540337457');\n" +
                "INSERT INTO num_data VALUES (0, '0');\n" +
                "INSERT INTO num_data VALUES (1, '0');\n" +
                "INSERT INTO num_data VALUES (2, '-34338492.215397047');\n" +
                "INSERT INTO num_data VALUES (3, '4.31');\n" +
                "INSERT INTO num_data VALUES (4, '7799461.4119');\n" +
                "INSERT INTO num_data VALUES (5, '16397.038491');\n" +
                "INSERT INTO num_data VALUES (6, '93901.57763026');\n" +
                "INSERT INTO num_data VALUES (7, '-83028485');\n" +
                "INSERT INTO num_data VALUES (8, '74881');\n" +
                "INSERT INTO num_data VALUES (9, '-24926804.045047420');";
        compiler.compileStatements(createTables);
        compiler.compileStatements(insert);
    }

    /**
     * @param intermediate  A SQL query that defines an intermediate view, which is not output by the circuit.
     * @param last          A SQL query that defines the final view, which is output from the circuit.
     */
    public void testTwoViews(String intermediate, String last) {
        DBSPCompiler compiler = new DBSPCompiler(this.getOptions(true));
        this.prepareData(compiler);
        compiler.generateOutputForNextView(false);
        compiler.compileStatement(intermediate);
        compiler.generateOutputForNextView(true);
        compiler.compileStatement(last);
        compiler.optimize();
        DBSPCircuit circuit = getCircuit(compiler);
        InputOutputPair streams = new InputOutputPair(
                this.getPreparedInputs(compiler),
                new DBSPZSetLiteral.Contents[] {
                        DBSPZSetLiteral.Contents.emptyWithElementType(new DBSPTypeTuple(
                    DBSPTypeInteger.SIGNED_32,
                    DBSPTypeInteger.SIGNED_64,
                    new DBSPTypeDecimal(CalciteObject.EMPTY, WIDTH, 10, false),
                    new DBSPTypeDecimal(CalciteObject.EMPTY, WIDTH, 10, false))
                )}
        );
        this.addRustTestCase(last, compiler, circuit, streams);
    }

    @Test
    public void testAdd() {
        String intermediate = "CREATE VIEW num_result AS SELECT t1.id AS ID1, t2.id as ID2, "  +
                "CAST(t1.val + t2.val AS NUMERIC(" + WIDTH + ", 10)) AS results\n" +
                "    FROM num_data t1, num_data t2";
        String last = "CREATE VIEW E AS SELECT t1.id1, t1.id2, t1.results, t2.expected\n" +
                "    FROM num_result t1, num_exp_add t2\n" +
                "    WHERE t1.id1 = t2.id1 AND t1.id2 = t2.id2\n" +
                "    AND t1.results != t2.expected";
        this.testTwoViews(intermediate, last);
    }

    @Test
    public void testRoundAdd() {
        String intermediate = "CREATE VIEW num_result AS SELECT t1.id AS ID1, t2.id AS ID2, " +
                "CAST(round(t1.val + t2.val, 10) AS NUMERIC(" + WIDTH + ", 10)) AS results\n" +
                "    FROM num_data t1, num_data t2";
        String last = "CREATE VIEW E AS SELECT t1.id1, t1.id2, t1.results, round(t2.expected, 10) as expected\n" +
                "    FROM num_result t1, num_exp_add t2\n" +
                "    WHERE t1.id1 = t2.id1 AND t1.id2 = t2.id2\n" +
                "    AND t1.results != round(t2.expected, 10)";
        this.testTwoViews(intermediate, last);
    }

    @Test
    public void testSubtraction() {
        String intermediate = "CREATE VIEW num_result AS SELECT t1.id AS ID1, t2.id AS ID2, " +
                "CAST(t1.val - t2.val AS NUMERIC(" + WIDTH + ", 10)) AS results\n" +
                "    FROM num_data t1, num_data t2";
        String last = "CREATE VIEW E AS SELECT t1.id1, t1.id2, t1.results, t2.expected\n" +
                "    FROM num_result t1, num_exp_sub t2\n" +
                "    WHERE t1.id1 = t2.id1 AND t1.id2 = t2.id2\n" +
                "    AND t1.results != t2.expected";
        this.testTwoViews(intermediate, last);
    }

    @Test
    public void testRoundSubtraction() {
        String intermediate = "CREATE VIEW num_result AS SELECT t1.id AS ID1, t2.id AS ID2, " +
                "CAST(ROUND(t1.val - t2.val, 40) AS NUMERIC(" + WIDTH + ",10)) AS results\n" +
                "    FROM num_data t1, num_data t2";
        String last = "CREATE VIEW E AS SELECT t1.id1, t1.id2, t1.results, round(t2.expected, 40)\n" +
                "    FROM num_result t1, num_exp_sub t2\n" +
                "    WHERE t1.id1 = t2.id1 AND t1.id2 = t2.id2\n" +
                "    AND t1.results != ROUND(t2.expected, 40)";
        this.testTwoViews(intermediate, last);
    }

    @Test
    public void testMultiply() {
        String intermediate = "CREATE VIEW num_result AS SELECT t1.id AS ID1, t2.id AS ID2, " +
                "CAST(t1.val * t2.val AS NUMERIC(" + WIDTH + ",10)) AS results\n" +
                "    FROM num_data t1, num_data t2";
        String last = "CREATE VIEW E AS SELECT t1.id1, t1.id2, t1.results, t2.expected\n" +
                "    FROM num_result t1, num_exp_mul t2\n" +
                "    WHERE t1.id1 = t2.id1 AND t1.id2 = t2.id2\n" +
                "    AND t1.results != t2.expected";
        this.testTwoViews(intermediate, last);
    }

    @Test
    public void testRoundMultiply() {
        String intermediate = "CREATE VIEW num_result AS SELECT t1.id AS ID1, t2.id AS ID2, " +
                "CAST(round(t1.val * t2.val, 30) AS NUMERIC(" + WIDTH + ", 10)) AS results\n" +
                "    FROM num_data t1, num_data t2";
        String last = "CREATE VIEW E AS SELECT t1.id1, t1.id2, t1.results, round(t2.expected, 30)\n" +
                "    FROM num_result t1, num_exp_mul t2\n" +
                "    WHERE t1.id1 = t2.id1 AND t1.id2 = t2.id2\n" +
                "    AND t1.results != round(t2.expected, 30)";
        this.testTwoViews(intermediate, last);
    }

    @Test
    public void testDivision() {
        String intermediate = "CREATE VIEW num_result AS SELECT t1.id AS ID1, t2.id AS ID2, " +
                "CAST(t1.val / t2.val AS NUMERIC(" + WIDTH + ", 10)) AS results\n" +
                "    FROM num_data t1, num_data t2";
        String last = "CREATE VIEW E AS SELECT t1.id1, t1.id2, t1.results, t2.expected\n" +
                "    FROM num_result t1, num_exp_div t2\n" +
                "    WHERE t1.id1 = t2.id1 AND t1.id2 = t2.id2\n" +
                "    AND t1.results != t2.expected";
        this.testTwoViews(intermediate, last);
    }

    @Test
    public void testDivisionRound() {
        String intermediate = "CREATE VIEW num_result AS SELECT t1.id AS ID1, t2.id AS ID2, " +
                "CAST(round(t1.val / t2.val, 10) AS NUMERIC(" + WIDTH + ", 10)) AS results\n" +
                "    FROM num_data t1, num_data t2";
        String last = "CREATE VIEW E AS SELECT t1.id1, t1.id2, t1.results, round(t2.expected, 10)\n" +
                "    FROM num_result t1, num_exp_div t2\n" +
                "    WHERE t1.id1 = t2.id1 AND t1.id2 = t2.id2\n" +
                "    AND t1.results != round(t2.expected, 10)";
        this.testTwoViews(intermediate, last);
    }

    @Test
    public void squareRootTest() {
        String intermediate = "CREATE VIEW num_result AS SELECT id AS ID1, 0 as ID2, CAST(SQRT(ABS(val)) AS NUMERIC(" +
                WIDTH + ", 10)) AS results\n" +
                "    FROM num_data\n";
        String last = "CREATE VIEW E AS SELECT t1.id1, t1.results, t2.expected\n" +
                "    FROM num_result t1, num_exp_sqrt t2\n" +
                "    WHERE t1.id1 = t2.id\n" +
                "    AND t1.results != t2.expected";
        this.testTwoViews(intermediate, last);
    }

    @Test
    public void logarithmTest() {
        String intermediate = "CREATE VIEW num_result AS SELECT id AS ID1, 0, CAST(LN(ABS(val)) AS NUMERIC(" +
                WIDTH + ", 10)) AS results\n" +
                "    FROM num_data\n" +
                "    WHERE val != '0.0'";
        String last = "CREATE VIEW E AS SELECT t1.id1, t1.results, t2.expected\n" +
                "    FROM num_result t1, num_exp_ln t2\n" +
                "    WHERE t1.id1 = t2.id\n" +
                "    AND t1.results != t2.expected";
        this.testTwoViews(intermediate, last);
    }

    @Test
    public void logarithm10Test() {
        String intermediate = "CREATE VIEW num_result AS SELECT id AS ID1, 0, CAST(LOG10(ABS(val)) AS NUMERIC(" +
                WIDTH + ", 10)) AS results\n" +
                "    FROM num_data\n" +
                "    WHERE val != '0.0'";
        String last = "CREATE VIEW E AS SELECT t1.id1, t1.results, t2.expected\n" +
                "    FROM num_result t1, num_exp_log10 t2\n" +
                "    WHERE t1.id1 = t2.id\n" +
                "    AND t1.results != t2.expected";
        this.testTwoViews(intermediate, last);
    }

    @Test @Ignore("This test fails because Postgres has higher precision than we support")
    public void power10Test() {
        String intermediate = "CREATE VIEW num_result AS SELECT id AS ID1, 0, " +
                "CAST(POWER(10, LN(ABS(round(val,200)))) AS NUMERIC(" +
                WIDTH + ", 10)) AS results\n" +
                "    FROM num_data\n" +
                "    WHERE val != '0.0'";
        String last = "CREATE VIEW E AS SELECT t1.id1, t1.results, t2.expected\n" +
                "    FROM num_result t1, num_exp_power_10_ln t2\n" +
                "    WHERE t1.id1 = t2.id\n" +
                "    AND t1.results != t2.expected";
        this.testTwoViews(intermediate, last);
    }

    @Test
    public void testSpecialValues() {
        // This test was written with NUMERIC values, but was converted to FP
        this.q(
                "WITH v(x) AS (VALUES(0E0),(1E0),(-1E0),(4.2E0),(CAST ('Infinity' AS DOUBLE))," +
                        "(CAST ('-Infinity' AS DOUBLE)),(CAST ('nan' AS DOUBLE)))\n" +
                "SELECT x1, x2,\n" +
                "  x1 + x2 AS s,\n" +
                "  x1 - x2 AS diff,\n" +
                "  x1 * x2 AS prod\n" +
                "FROM v AS v1(x1), v AS v2(x2);\n" +
                "    x1     |    x2     |    sum    |   diff    |   prod    \n" +
                "-----------+-----------+-----------+-----------+-----------\n" +
                "         0 |         0 |         0 |         0 |         0\n" +
                "         0 |         1 |         1 |        -1 |         0\n" +
                "         0 |        -1 |        -1 |         1 |         0\n" +
                "         0 |       4.2 |       4.2 |      -4.2 |       0.0\n" +
                "         0 |  Infinity |  Infinity | -Infinity |       NaN\n" +
                "         0 | -Infinity | -Infinity |  Infinity |       NaN\n" +
                "         0 |       NaN |       NaN |       NaN |       NaN\n" +
                "         1 |         0 |         1 |         1 |         0\n" +
                "         1 |         1 |         2 |         0 |         1\n" +
                "         1 |        -1 |         0 |         2 |        -1\n" +
                "         1 |       4.2 |       5.2 |      -3.2 |       4.2\n" +
                "         1 |  Infinity |  Infinity | -Infinity |  Infinity\n" +
                "         1 | -Infinity | -Infinity |  Infinity | -Infinity\n" +
                "         1 |       NaN |       NaN |       NaN |       NaN\n" +
                "        -1 |         0 |        -1 |        -1 |         0\n" +
                "        -1 |         1 |         0 |        -2 |        -1\n" +
                "        -1 |        -1 |        -2 |         0 |         1\n" +
                "        -1 |       4.2 |       3.2 |      -5.2 |      -4.2\n" +
                "        -1 |  Infinity |  Infinity | -Infinity | -Infinity\n" +
                "        -1 | -Infinity | -Infinity |  Infinity |  Infinity\n" +
                "        -1 |       NaN |       NaN |       NaN |       NaN\n" +
                "       4.2 |         0 |       4.2 |       4.2 |       0.0\n" +
                "       4.2 |         1 |       5.2 |       3.2 |       4.2\n" +
                "       4.2 |        -1 |       3.2 |       5.2 |      -4.2\n" +
                "       4.2 |       4.2 |       8.4 |       0.0 |     17.64\n" +
                "       4.2 |  Infinity |  Infinity | -Infinity |  Infinity\n" +
                "       4.2 | -Infinity | -Infinity |  Infinity | -Infinity\n" +
                "       4.2 |       NaN |       NaN |       NaN |       NaN\n" +
                "  Infinity |         0 |  Infinity |  Infinity |       NaN\n" +
                "  Infinity |         1 |  Infinity |  Infinity |  Infinity\n" +
                "  Infinity |        -1 |  Infinity |  Infinity | -Infinity\n" +
                "  Infinity |       4.2 |  Infinity |  Infinity |  Infinity\n" +
                "  Infinity |  Infinity |  Infinity |       NaN |  Infinity\n" +
                "  Infinity | -Infinity |       NaN |  Infinity | -Infinity\n" +
                "  Infinity |       NaN |       NaN |       NaN |       NaN\n" +
                " -Infinity |         0 | -Infinity | -Infinity |       NaN\n" +
                " -Infinity |         1 | -Infinity | -Infinity | -Infinity\n" +
                " -Infinity |        -1 | -Infinity | -Infinity |  Infinity\n" +
                " -Infinity |       4.2 | -Infinity | -Infinity | -Infinity\n" +
                " -Infinity |  Infinity |       NaN | -Infinity | -Infinity\n" +
                " -Infinity | -Infinity | -Infinity |       NaN |  Infinity\n" +
                " -Infinity |       NaN |       NaN |       NaN |       NaN\n" +
                "       NaN |         0 |       NaN |       NaN |       NaN\n" +
                "       NaN |         1 |       NaN |       NaN |       NaN\n" +
                "       NaN |        -1 |       NaN |       NaN |       NaN\n" +
                "       NaN |       4.2 |       NaN |       NaN |       NaN\n" +
                "       NaN |  Infinity |       NaN |       NaN |       NaN\n" +
                "       NaN | -Infinity |       NaN |       NaN |       NaN\n" +
                "       NaN |       NaN |       NaN |       NaN |       NaN");
    }

    @Test @Ignore("https://issues.apache.org/jira/browse/CALCITE-5795")
    public void testCast() {
        this.q(
                "WITH v(x) AS (VALUES(0::numeric),(4.2)) SELECT x FROM v as v1(x);\n" +
                "    x1     \n" +
                "-----------\n" +
                "         0 \n" +
                "       4.2 \n");
    }

    @Test
    public void testSpecialValuesNumeric() {
        // Removed unsupported numeric values inf, nan, etc.
        this.q(
                "WITH v(x) AS (VALUES('0'::numeric),(1),(-1),('4.2'::numeric))\n" +
                        "SELECT x1, x2,\n" +
                        "  x1 + x2 AS s,\n" +
                        "  x1 - x2 AS diff,\n" +
                        "  x1 * x2 AS prod\n" +
                        "FROM v AS v1(x1), v AS v2(x2);\n" +
                "    x1     |    x2     |    sum    |   diff    |   prod    \n" +
                "-----------+-----------+-----------+-----------+-----------\n" +
                "         0 |         0 |         0 |         0 |         0\n" +
                "         0 |         1 |         1 |        -1 |         0\n" +
                "         0 |        -1 |        -1 |         1 |         0\n" +
                "         0 |       4.2 |       4.2 |      -4.2 |       0.0\n" +
                "         1 |         0 |         1 |         1 |         0\n" +
                "         1 |         1 |         2 |         0 |         1\n" +
                "         1 |        -1 |         0 |         2 |        -1\n" +
                "         1 |       4.2 |       5.2 |      -3.2 |       4.2\n" +
                "        -1 |         0 |        -1 |        -1 |         0\n" +
                "        -1 |         1 |         0 |        -2 |        -1\n" +
                "        -1 |        -1 |        -2 |         0 |         1\n" +
                "        -1 |       4.2 |       3.2 |      -5.2 |      -4.2\n" +
                "       4.2 |         0 |       4.2 |       4.2 |       0.0\n" +
                "       4.2 |         1 |       5.2 |       3.2 |       4.2\n" +
                "       4.2 |        -1 |       3.2 |       5.2 |      -4.2\n" +
                "       4.2 |       4.2 |       8.4 |       0.0 |     17.64");
    }

    @Test
    public void testSpecialValues2() {
        // no div or mod defined for fp, so I removed these
        this.q("WITH v(x) AS\n" +
                "  (VALUES(0E0),(1E0),(-1E0),(4.2E0),(CAST ('Infinity' AS DOUBLE)),(CAST ('-Infinity' AS DOUBLE))," +
                "(CAST ('nan' AS DOUBLE)))\n" +
                "SELECT x1, x2,\n" +
                "  x1 / x2 AS quot\n" +
                //"  x1 % x2 AS m,\n" +
                //"  div(x1, x2) AS div\n" +
                "FROM v AS v1(x1), v AS v2(x2) WHERE x2 != 0E0;\n" +
                "    x1     |    x2     |          quot            \n" +
                "-----------+-----------+--------------------------\n" +
                "         0 |         1 |  0.00000000000000000000 \n" +
                "         1 |         1 |  1.00000000000000000000 \n" +
                "        -1 |         1 | -1.00000000000000000000 \n" +
                "       4.2 |         1 |      4.2000000000000000 \n" +
                "  Infinity |         1 |                Infinity \n" +
                " -Infinity |         1 |               -Infinity \n" +
                "       NaN |         1 |                     NaN \n" +
                "         0 |        -1 |  0.00000000000000000000 \n" +
                "         1 |        -1 | -1.00000000000000000000 \n" +
                "        -1 |        -1 |  1.00000000000000000000 \n" +
                "       4.2 |        -1 |     -4.2000000000000000 \n" +
                "  Infinity |        -1 |               -Infinity \n" +
                " -Infinity |        -1 |                Infinity \n" +
                "       NaN |        -1 |                     NaN \n" +
                "         0 |       4.2 |  0.00000000000000000000 \n" +
                "         1 |       4.2 |  0.23809523809523809524 \n" +
                "        -1 |       4.2 | -0.23809523809523809524 \n" +
                "       4.2 |       4.2 |  1.00000000000000000000 \n" +
                "  Infinity |       4.2 |                Infinity \n" +
                " -Infinity |       4.2 |               -Infinity \n" +
                "       NaN |       4.2 |                     NaN \n" +
                "         0 |  Infinity |                       0 \n" +
                "         1 |  Infinity |                       0 \n" +
                "        -1 |  Infinity |                       0 \n" +
                "       4.2 |  Infinity |                       0 \n" +
                "  Infinity |  Infinity |                     NaN \n" +
                " -Infinity |  Infinity |                     NaN \n" +
                "       NaN |  Infinity |                     NaN \n" +
                "         0 | -Infinity |                       0 \n" +
                "         1 | -Infinity |                       0 \n" +
                "        -1 | -Infinity |                       0 \n" +
                "       4.2 | -Infinity |                       0 \n" +
                "  Infinity | -Infinity |                     NaN \n" +
                " -Infinity | -Infinity |                     NaN \n" +
                "       NaN | -Infinity |                     NaN \n" +
                "         0 |       NaN |                     NaN \n" +
                "         1 |       NaN |                     NaN \n" +
                "        -1 |       NaN |                     NaN \n" +
                "       4.2 |       NaN |                     NaN \n" +
                "  Infinity |       NaN |                     NaN \n" +
                " -Infinity |       NaN |                     NaN \n" +
                "       NaN |       NaN |                     NaN ");
    }

    @Test @Ignore("https://issues.apache.org/jira/browse/CALCITE-5651")
    public void testSpecialValues2Numeric() {
        // Removed unsupported numeric values inf, nan, etc.
        // No div function known, so I removed this one
        this.q("WITH v(x) AS\n" +
                "  (VALUES(CAST(0 AS NUMERIC(" + WIDTH + ", 20))),\n" +
                "         (CAST(1 AS NUMERIC(" + WIDTH + ", 20))),\n" +
                "         (CAST(-1 AS NUMERIC(" + WIDTH + ",20))),\n" +
                "         (CAST(4.2 AS NUMERIC(" + WIDTH + ", 20))))\n" +
                "SELECT x1, x2,\n" +
                "  x1 / x2 AS quot,\n" +
                "  x1 % x2 AS m\n" +
                // "  div(x1, x2) AS div\n" +
                "FROM v AS v1(x1), v AS v2(x2) WHERE x2 != 0;\n" +
                "    x1     |    x2     |          quot           | mod  \n" +
                "-----------+-----------+-------------------------+------\n" +
                "         0 |         1 |  0.00000000000000000000 |    0\n" +
                "         1 |         1 |  1.00000000000000000000 |    0\n" +
                "        -1 |         1 | -1.00000000000000000000 |    0\n" +
                "       4.2 |         1 |      4.2000000000000000 |  0.2\n" +
                "         0 |        -1 |  0.00000000000000000000 |    0\n" +
                "         1 |        -1 | -1.00000000000000000000 |    0\n" +
                "        -1 |        -1 |  1.00000000000000000000 |    0\n" +
                "       4.2 |        -1 |     -4.2000000000000000 |  0.2\n" +
                "         0 |       4.2 |  0.00000000000000000000 |  0.0\n" +
                "         1 |       4.2 |  0.23809523809523809524 |  1.0\n" +
                "        -1 |       4.2 | -0.23809523809523809524 | -1.0\n" +
                "       4.2 |       4.2 |  1.00000000000000000000 |  0.0\n");
    }

    // We don't support 'Infinity' for Decimal
    // We don't support NaN for Decimal
    //SELECT '0'::numeric / '0';
    //ERROR:  division by zero
    //SELECT 'inf'::numeric % '0';
    //ERROR:  division by zero
    //SELECT '-inf'::numeric % '0';
    //ERROR:  division by zero
    //SELECT 'nan'::numeric % '0';
    // ?column?
    //----------
    //      NaN
    //(1 row)
    //
    //SELECT '0'::numeric % '0';
    //ERROR:  division by zero
    //SELECT div('inf'::numeric, '0');
    //ERROR:  division by zero
    //SELECT div('-inf'::numeric, '0');
    //ERROR:  division by zero
    //SELECT div('nan'::numeric, '0');
    // div
    //-----
    // NaN
    //(1 row)
    //
    //SELECT div('0'::numeric, '0');
    //ERROR:  division by zero

    @Test
    public void testFunctions0() {
        // TODO: this test was written with NUMERIC values, but was converted to FP
        this.q("WITH v(x) AS\n" +
                "  (VALUES(0E0),(1E0),(-1E0),(4.2E0),(-7.777E0),(CAST('inf' AS DOUBLE)),(CAST('-inf' AS DOUBLE)),(CAST('nan' AS DOUBLE)))\n" +
                "SELECT x, -x as minusx, abs(x), floor(x), ceil(x), sign(x)\n" +
                "FROM v;\n" +
                "     x     |  minusx   |   abs    |   floor   |   ceil    | sign  \n" +
                "-----------+-----------+----------+-----------+-----------+-------\n" +
                "         0 |         0 |        0 |         0 |         0 |    0 \n" +
                "         1 |        -1 |        1 |         1 |         1 |    1 \n" +
                "        -1 |         1 |        1 |        -1 |        -1 |   -1 \n" +
                "       4.2 |      -4.2 |      4.2 |         4 |         5 |    1 \n" +
                "    -7.777 |     7.777 |    7.777 |        -8 |        -7 |   -1 \n" +
                "  Infinity | -Infinity | Infinity |  Infinity |  Infinity |    1 \n" +
                " -Infinity |  Infinity | Infinity | -Infinity | -Infinity |   -1 \n" +
                "       NaN |       NaN |      NaN |       NaN |       NaN |  NaN ");
    }

    @Test
    public void testFunctionsNumeric0() {
        // dropped unsupported values inf, nan, etc.
        this.q("WITH v(x) AS\n" +
                "  (VALUES(0),(1),(-1),(4.2),(-7.777))\n" +
                "SELECT x, -x as minusx, abs(x), floor(x), ceil(x), sign(x)\n" +
                "FROM v;\n" +
                "     x     |  minusx   |   abs    |   floor   |   ceil    | sign \n" +
                "-----------+-----------+----------+-----------+-----------+------\n" +
                "         0 |         0 |        0 |         0 |         0 |    0\n" +
                "         1 |        -1 |        1 |         1 |         1 |    1\n" +
                "        -1 |         1 |        1 |        -1 |        -1 |   -1\n" +
                "       4.2 |      -4.2 |      4.2 |         4 |         5 |    1\n" +
                "    -7.777 |     7.777 |    7.777 |        -8 |        -7 |   -1");
    }

    @Test
    public void testFunctions1() {
        // Removed the unsupported inf, nan, etc. values
        // This test makes not sense for FP
        // 'trunc' has been renamed to 'truncate'
        this.q("WITH v(x) AS\n" +
                "  (VALUES(0),(1),(-1),(4.2),(-7.777))\n" +
                "SELECT x, round(x), round(x,1) as round1, truncate(x), truncate(x,1) as trunc1\n" +
                "FROM v;\n" +
                "     x     |   round   |  round1   |   trunc   |  trunc1   \n" +
                "-----------+-----------+-----------+-----------+-----------\n" +
                "         0 |         0 |       0.0 |         0 |       0.0\n" +
                "         1 |         1 |       1.0 |         1 |       1.0\n" +
                "        -1 |        -1 |      -1.0 |        -1 |      -1.0\n" +
                "       4.2 |         4 |       4.2 |         4 |       4.2\n" +
                "    -7.777 |        -8 |      -7.8 |        -7 |      -7.7");
    }

    // -- the large values fall into the numeric abbreviation code's maximal classes
    //WITH v(x) AS
    //  (VALUES('0'::numeric),('1'),('-1'),('4.2'),('-7.777'),('1e340'),('-1e340'),
    //         ('inf'),('-inf'),('nan'),
    //         ('inf'),('-inf'),('nan'))
    //SELECT substring(x::text, 1, 32)
    //FROM v ORDER BY x;
    //            substring
    //----------------------------------
    // -Infinity
    // -Infinity
    // -1000000000000000000000000000000
    // -7.777
    // -1
    // 0
    // 1
    // 4.2
    // 10000000000000000000000000000000
    // Infinity
    // Infinity
    // NaN
    // NaN
    //(13 rows)

    @Test
    public void testSqrt() {
        // Removed 'inf' and 'nan'.
        // Interestingly, sqrt in Calcite returns a FP value.
        this.q("WITH v(x) AS\n" +
                "  (VALUES(0),(1),(4.2))\n" +
                "SELECT x, sqrt(x)\n" +
                "FROM v;\n" +
                "    x     |       sqrt        \n" +
                "----------+-------------------\n" +
                "        0 | 0.000000000000000\n" +
                "        1 | 1.000000000000000\n" +
                "      4.2 | 2.049390153191920");
    }

    // TODO: Calcite thinks that sqrt(-1) should produce a runtime error
    // SELECT sqrt('-1'::numeric);
    //ERROR:  cannot take square root of a negative number

    @Test @Ignore("Waiting for https://issues.apache.org/jira/browse/CALCITE-5651")
    public void testLog() {
        // Removed 'inf' and 'nan'
        // Calcite does not have log
        this.q("WITH v(x) AS\n" +
                "  (VALUES(1),(CAST(4.2 AS NUMERIC(" + WIDTH + ", 22))))\n" +
                "SELECT x,\n" +
                //"  log(x),\n" +
                "  log10(x),\n" +
                "  ln(x)\n" +
                "FROM v;\n" +
                "    x     |        log         |         ln         \n" +
                "----------+--------------------+--------------------\n" +
                "        1 | 0.0000000000000000 | 0.0000000000000000\n" +
                "      4.2 | 0.6232492903979005 | 1.4350845252893226");
    }
}
