/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

package com.vaticle.typedb.core.reasoner.benchmark.iam.complex;

import com.vaticle.typedb.core.common.diagnostics.Diagnostics;
import com.vaticle.typedb.core.reasoner.benchmark.iam.common.Benchmark;
import com.vaticle.typedb.core.reasoner.benchmark.iam.common.BenchmarkRunner;
import com.vaticle.typedb.core.reasoner.benchmark.iam.common.QueryParams;
import org.junit.After;
import org.junit.AfterClass;
import org.junit.BeforeClass;
import org.junit.Test;

import java.io.IOException;
import java.nio.file.Path;
import java.nio.file.Paths;

public class ConjunctionStructureTest {
    static final Path RESOURCE_DIRECTORY = Paths.get("test", "benchmark", "reasoner", "iam", "complex");
    private static final Path COMMON_RESOURCE_DIR = Paths.get("test", "benchmark", "reasoner", "iam", "resources");

    private static final String database = "iam-benchmark-conjunctions";
    private static final BenchmarkRunner benchmarker = new BenchmarkRunner(database);
    private final QueryParams queryParams;

    public ConjunctionStructureTest() {
        queryParams = QueryParams.load(COMMON_RESOURCE_DIR.resolve("params.yml"));
    }

    @BeforeClass
    public static void setup() throws IOException {
        Diagnostics.Noop.initialise();
        benchmarker.setUp();
        benchmarker.loadDatabase(COMMON_RESOURCE_DIR.resolve("types.tql"), COMMON_RESOURCE_DIR.resolve("data.typedb"));
        benchmarker.loadSchema(COMMON_RESOURCE_DIR.resolve("rules.tql"));
        benchmarker.warmUp();
    }

    @AfterClass
    public static void tearDown() {
        benchmarker.tearDown();
    }

    @After
    public void reset() {
        benchmarker.reset();
    }

    @Test
    public void testMultipleStartingPoints() {
        String query = String.format(
                "match\n" +
                        "   $s isa subject, has email \"%s\";\n" +
                        "   $parent isa directory, has path \"%s\";\n" +
                        "   $a1 isa action, has name \"%s\";\n" +
                        "   $a2 isa action, has name \"%s\";\n" +
                        "   $policy (action: $a1, action: $a2) isa segregation-policy;\n" +
                        "   (collection: $parent, member:$o) isa collection-membership;\n" +
                        "   $o has id $oid;" +
                        "   $ac1(object: $o, action: $a1) isa access;\n" +
                        "   $ac2(object: $o, action: $a2) isa access;\n" +
                        "   $p1 (subject: $s, access: $ac1) isa permission;\n" +
                        "   $p2 (subject: $s, access: $ac2) isa permission;\n" +
                "get $oid;",
                queryParams.segregationEmail, queryParams.segregationObject, queryParams.segregationAction1, queryParams.segregationAction2);
        Benchmark benchmark = new Benchmark("multiple-starting-points", query, 1);
        benchmarker.runBenchmark(benchmark);

        benchmark.assertAnswerCountCorrect();
        benchmark.assertRunningTime(1000);
        benchmark.assertCounters(500, 17, 50, 60, 150);
    }

    @Test
    public void testHighArityBounds() {
        benchmarker.loadSchema(RESOURCE_DIRECTORY.resolve("conjunction-structure-test.tql"));
        String query = String.format(
                "match\n" +
                        "   $s isa subject, has email \"%s\";\n" +
                        "   $parent isa directory, has path \"%s\";\n" +
                        "   $a1 isa action, has name \"%s\";\n" +
                        "   $a2 isa action, has name \"%s\";\n" +
                        "   (collection: $parent, member:$o) isa collection-membership;\n" +
                        "   (subject: $s, object: $o, action: $a1, action: $a2) isa high-arity-test-segregation-violation;\n" +
                "get;",
                queryParams.segregationEmail, queryParams.segregationObject, queryParams.segregationAction1, queryParams.segregationAction2);

        Benchmark benchmark = new Benchmark("high-arity-bounds", query, 1);
        benchmarker.runBenchmark(benchmark);

        benchmark.assertAnswerCountCorrect();
        benchmark.assertRunningTime(1000);
        benchmark.assertCounters(500, 22, 69, 102, 165);
    }
}
