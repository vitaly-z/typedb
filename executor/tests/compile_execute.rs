/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use compiler::{
    annotation::{
        expression::block_compiler::compile_expressions, function::EmptyAnnotatedFunctionSignatures,
        match_inference::infer_types,
    },
    executable::{
        function::ExecutableFunctionRegistry, match_::planner::conjunction_executable::ConjunctionExecutable,
    },
};
use concept::{
    thing::{statistics::Statistics, thing_manager::ThingManager},
    type_::type_manager::TypeManager,
};
use encoding::graph::definition::definition_key_generator::DefinitionKeyGenerator;
use executor::{
    conjunction_executor::ConjunctionExecutor, pipeline::stage::ExecutionContext, row::MaybeOwnedRow,
    ExecutionInterrupt,
};
use function::function_manager::FunctionManager;
use ir::{
    pipeline::{function_signature::HashMapFunctionSignatureIndex, ParameterRegistry},
    translation::{match_::translate_match, PipelineTranslationContext},
};
use itertools::Itertools;
use lending_iterator::LendingIterator;
use query::query_manager::QueryManager;
use resource::profile::{CommitProfile, QueryProfile};
use storage::{
    durability_client::WALClient,
    sequence_number::SequenceNumber,
    snapshot::{CommittableSnapshot, ReadableSnapshot},
    MVCCStorage,
};
use test_utils::assert_matches;
use test_utils_concept::{load_managers, setup_concept_storage};
use test_utils_encoding::create_core_storage;

fn setup(
    storage: &Arc<MVCCStorage<WALClient>>,
    type_manager: Arc<TypeManager>,
    thing_manager: Arc<ThingManager>,
    schema: &str,
    data: &str,
) -> Statistics {
    std::panic::set_hook({
        let default_panic = std::panic::take_hook();
        Box::new(move |info| {
            default_panic(info);
            std::process::exit(1);
        })
    });

    let query_manager = QueryManager::new(None);
    let function_manager = FunctionManager::new(Arc::new(DefinitionKeyGenerator::new()), None);
    let mut snapshot = storage.clone().open_snapshot_schema();
    let define = typeql::parse_query(schema).unwrap().into_structure().into_schema();
    query_manager
        .execute_schema(&mut snapshot, &type_manager, &thing_manager, &function_manager, define, schema)
        .unwrap();
    snapshot.commit(&mut CommitProfile::DISABLED).unwrap();

    let snapshot = storage.clone().open_snapshot_write();
    let query = typeql::parse_query(data).unwrap().into_structure().into_pipeline();
    let pipeline = query_manager
        .prepare_write_pipeline(
            snapshot,
            &type_manager,
            thing_manager.clone(),
            &FunctionManager::default(),
            &query,
            data,
        )
        .unwrap();
    let (mut iterator, ExecutionContext { snapshot, .. }) =
        pipeline.into_rows_iterator(ExecutionInterrupt::new_uninterruptible()).unwrap();
    assert_matches!(iterator.next(), Some(Ok(_)));
    assert_matches!(iterator.next(), None);
    let snapshot = Arc::into_inner(snapshot).unwrap();
    snapshot.commit(&mut CommitProfile::DISABLED).unwrap();

    let mut statistics = Statistics::new(SequenceNumber::new(0));
    statistics.may_synchronise(storage).unwrap();
    statistics
}

#[test]
fn test_has_planning_traversal() {
    let (_tmp_dir, mut storage) = create_core_storage();
    setup_concept_storage(&mut storage);
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let schema = "define
        attribute age value integer;
        attribute name value string;
        entity person owns age @card(0..), owns name @card(0..);
    ";
    let data = "insert
        $_ isa person, has age 10, has age 11, has age 12, has name 'John', has name 'Alice';
        $_ isa person, has age 10, has age 13, has age 14;
        $_ isa person, has age 13, has name 'Leila';
    ";

    let statistics = setup(&storage, type_manager, thing_manager, schema, data);

    let query = "match $person isa person, has name $name, has age $age;";
    let match_ = typeql::parse_query(query).unwrap().into_structure().into_pipeline().stages.remove(0).into_match();

    // IR
    let empty_function_index = HashMapFunctionSignatureIndex::empty();
    let mut translation_context = PipelineTranslationContext::new();
    let mut value_parameters = ParameterRegistry::new();
    let builder =
        translate_match(&mut translation_context, &mut value_parameters, &empty_function_index, &match_).unwrap();
    let block = builder.finish().unwrap();

    // Executor
    let snapshot = Arc::new(storage.clone().open_snapshot_read());
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let entry_annotations = infer_types(
        &*snapshot,
        &block,
        &translation_context.variable_registry,
        &type_manager,
        &BTreeMap::new(),
        &EmptyAnnotatedFunctionSignatures,
        false,
    )
    .unwrap();

    let conjunction_executable = compiler::executable::match_::planner::compile(
        &block,
        &BTreeMap::new(),
        &HashMap::new(),
        &block.conjunction().named_producible_variables(block.block_context()).collect(),
        &entry_annotations,
        &translation_context.variable_registry,
        &HashMap::new(),
        &statistics,
        &ExecutableFunctionRegistry::empty(),
    )
    .unwrap();
    let executor = ConjunctionExecutor::new(
        &conjunction_executable,
        &snapshot,
        &thing_manager,
        MaybeOwnedRow::empty(),
        Arc::new(ExecutableFunctionRegistry::empty()),
        &QueryProfile::new(false),
    )
    .unwrap();

    let context = ExecutionContext::new(snapshot, thing_manager, Arc::default());
    let iterator = executor.into_iterator(context, ExecutionInterrupt::new_uninterruptible());

    let rows = iterator
        .map_static(|row| row.map(|row| row.into_owned()).map_err(|err| err.clone()))
        .into_iter()
        .unique_by(|res| res.as_ref().unwrap().row().to_vec())
        .try_collect::<_, Vec<_>, _>()
        .unwrap();

    for row in &rows {
        for value in row {
            print!("{}, ", value);
        }
        println!()
    }

    assert_eq!(rows.len(), 7);
}

#[test]
fn test_expression_planning_traversal() {
    let (_tmp_dir, mut storage) = create_core_storage();
    setup_concept_storage(&mut storage);
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let schema = "define
        attribute age value integer;
        entity person owns age @card(0..);
    ";
    let data = "insert
        $_ isa person, has age 10;
        $_ isa person, has age 12;
        $_ isa person, has age 14;
    ";

    let statistics = setup(&storage, type_manager, thing_manager, schema, data);

    let query = "match
        $person_1 isa person, has age $age_1;
        $person_2 isa person, has age == $age_2;
        let $age_2 = $age_1 + 2;
    ";
    let match_ = typeql::parse_query(query).unwrap().into_structure().into_pipeline().stages.remove(0).into_match();

    // IR
    let empty_function_index = HashMapFunctionSignatureIndex::empty();
    let mut translation_context = PipelineTranslationContext::new();
    let mut value_parameters = ParameterRegistry::new();
    let builder =
        translate_match(&mut translation_context, &mut value_parameters, &empty_function_index, &match_).unwrap();
    let block = builder.finish().unwrap();

    // Executor
    let snapshot = Arc::new(storage.clone().open_snapshot_read());
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let entry_annotations = infer_types(
        &*snapshot,
        &block,
        &translation_context.variable_registry,
        &type_manager,
        &BTreeMap::new(),
        &EmptyAnnotatedFunctionSignatures,
        false,
    )
    .unwrap();

    let compiled_expressions = compile_expressions(
        &*snapshot,
        &type_manager,
        &block,
        &mut translation_context.variable_registry,
        &value_parameters,
        &entry_annotations,
        &mut BTreeMap::new(),
    )
    .unwrap();

    let conjunction_executable = compiler::executable::match_::planner::compile(
        &block,
        &BTreeMap::new(),
        &HashMap::new(),
        &block.conjunction().named_producible_variables(block.block_context()).collect(),
        &entry_annotations,
        &translation_context.variable_registry,
        &compiled_expressions,
        &statistics,
        &ExecutableFunctionRegistry::empty(),
    )
    .unwrap();
    let executor = ConjunctionExecutor::new(
        &conjunction_executable,
        &snapshot,
        &thing_manager,
        MaybeOwnedRow::empty(),
        Arc::new(ExecutableFunctionRegistry::empty()),
        &QueryProfile::new(false),
    )
    .unwrap();

    let context = ExecutionContext::new(snapshot, thing_manager, Arc::new(value_parameters));
    let iterator = executor.into_iterator(context, ExecutionInterrupt::new_uninterruptible());

    let rows = iterator
        .map_static(|row| row.map(|row| row.into_owned()).map_err(|err| err.clone()))
        .into_iter()
        .unique_by(|res| res.as_ref().unwrap().row().to_vec())
        .try_collect::<_, Vec<_>, _>()
        .unwrap();

    for row in &rows {
        for value in row {
            print!("{}, ", value);
        }
        println!()
    }

    assert_eq!(rows.len(), 2);
}

#[test]
fn test_links_planning_traversal() {
    let (_tmp_dir, mut storage) = create_core_storage();
    setup_concept_storage(&mut storage);
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let schema = "define
        entity person owns name @card(0..), plays membership:member;
        relation membership relates member @card(0..);
        attribute name value string;
    ";
    let data = "insert
        $p0 isa person, has name 'John';
        $p1 isa person, has name 'Alice';
        $p2 isa person, has name 'Leila';
        (member: $p0) isa membership;
        (member: $p2) isa membership;
    ";

    let statistics = setup(&storage, type_manager, thing_manager, schema, data);

    let query = "match $person isa person, has name $name; $membership isa membership, links ($person);";
    let match_ = typeql::parse_query(query).unwrap().into_structure().into_pipeline().stages.remove(0).into_match();

    // IR
    let empty_function_index = HashMapFunctionSignatureIndex::empty();
    let mut translation_context = PipelineTranslationContext::new();
    let mut value_parameters = ParameterRegistry::new();
    let builder =
        translate_match(&mut translation_context, &mut value_parameters, &empty_function_index, &match_).unwrap();
    let block = builder.finish().unwrap();

    // Executor
    let snapshot = Arc::new(storage.clone().open_snapshot_read());
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let entry_annotations = infer_types(
        &*snapshot,
        &block,
        &translation_context.variable_registry,
        &type_manager,
        &BTreeMap::new(),
        &EmptyAnnotatedFunctionSignatures,
        false,
    )
    .unwrap();

    let conjunction_executable = compiler::executable::match_::planner::compile(
        &block,
        &BTreeMap::new(),
        &HashMap::new(),
        &block.conjunction().named_producible_variables(block.block_context()).collect(),
        &entry_annotations,
        &translation_context.variable_registry,
        &HashMap::new(),
        &statistics,
        &ExecutableFunctionRegistry::empty(),
    )
    .unwrap();
    let executor = ConjunctionExecutor::new(
        &conjunction_executable,
        &snapshot,
        &thing_manager,
        MaybeOwnedRow::empty(),
        Arc::new(ExecutableFunctionRegistry::empty()),
        &QueryProfile::new(false),
    )
    .unwrap();

    let context = ExecutionContext::new(snapshot, thing_manager, Arc::default());
    let iterator = executor.into_iterator(context, ExecutionInterrupt::new_uninterruptible());

    let rows = iterator
        .map_static(|row| row.map(|row| row.into_owned()).map_err(|err| err.clone()))
        .into_iter()
        .unique_by(|res| res.as_ref().unwrap().row().to_vec())
        .try_collect::<_, Vec<_>, _>()
        .unwrap();

    for row in &rows {
        for value in row {
            print!("{}, ", value);
        }
        println!()
    }

    assert_eq!(rows.len(), 2);
}

#[test]
fn test_links_intersection() {
    let (_tmp_dir, mut storage) = create_core_storage();
    setup_concept_storage(&mut storage);
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let schema = "define
        entity user plays purchase:buyer;
        entity order, owns status, owns timestamp, plays purchase:order;
        relation purchase relates buyer, relates order;
        attribute status, value string;
        attribute timestamp, value datetime;
    ";
    let data = "insert
        $u0 isa user; $u1 isa user; $u2 isa user;
        $o0 isa order, has status 'canceled', has timestamp 1970-01-01T00:00;
        $o1 isa order, has status 'dispatched', has timestamp 1970-01-01T00:00;
        $o2 isa order, has status 'paid', has timestamp 1970-01-01T00:00;
        (buyer: $u0, order: $o0) isa purchase;
        (buyer: $u0, order: $o0) isa purchase;
        (buyer: $u1, order: $o1) isa purchase;
    ";

    let statistics = setup(&storage, type_manager, thing_manager, schema, data);

    let query = "match
    $p isa purchase, links (order: $order, buyer: $buyer);
    $order has status $status;
    $order has timestamp $timestamp;";
    let match_ = typeql::parse_query(query).unwrap().into_structure().into_pipeline().stages.remove(0).into_match();

    // IR
    let empty_function_index = HashMapFunctionSignatureIndex::empty();
    let mut translation_context = PipelineTranslationContext::new();
    let mut value_parameters = ParameterRegistry::new();
    let builder =
        translate_match(&mut translation_context, &mut value_parameters, &empty_function_index, &match_).unwrap();
    let block = builder.finish().unwrap();

    // Executor
    let snapshot = Arc::new(storage.clone().open_snapshot_read());
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let entry_annotations = infer_types(
        &*snapshot,
        &block,
        &translation_context.variable_registry,
        &type_manager,
        &BTreeMap::new(),
        &EmptyAnnotatedFunctionSignatures,
        false,
    )
    .unwrap();

    let conjunction_executable = compiler::executable::match_::planner::compile(
        &block,
        &BTreeMap::new(),
        &HashMap::new(),
        &block.conjunction().named_producible_variables(block.block_context()).collect(),
        &entry_annotations,
        &translation_context.variable_registry,
        &HashMap::new(),
        &statistics,
        &ExecutableFunctionRegistry::empty(),
    )
    .unwrap();
    let executor = ConjunctionExecutor::new(
        &conjunction_executable,
        &snapshot,
        &thing_manager,
        MaybeOwnedRow::empty(),
        Arc::new(ExecutableFunctionRegistry::empty()),
        &QueryProfile::new(false),
    )
    .unwrap();

    let context = ExecutionContext::new(snapshot, thing_manager, Arc::default());
    let iterator = executor.into_iterator(context, ExecutionInterrupt::new_uninterruptible());

    let rows = iterator
        .map_static(|row| row.map(|row| row.into_owned()).map_err(|err| err.clone()))
        .into_iter()
        .unique_by(|res| res.as_ref().unwrap().row().to_vec())
        .try_collect::<_, Vec<_>, _>()
        .unwrap();

    for row in &rows {
        for value in row {
            print!("{}, ", value);
        }
        println!()
    }

    assert_eq!(rows.len(), 3);
}

#[test]
fn test_negation_planning_traversal() {
    let (_tmp_dir, mut storage) = create_core_storage();
    setup_concept_storage(&mut storage);
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let schema = "define
        attribute age value integer;
        attribute name value string;
        entity person owns age @card(0..), owns name @card(0..);
    ";
    let data = "insert
        $_ isa person, has age 10, has age 11, has age 12, has name 'John', has name 'Alice';
        $_ isa person, has age 10, has age 13, has age 14;
        $_ isa person, has age 13, has name 'Leila';
    ";

    let statistics = setup(&storage, type_manager, thing_manager, schema, data);

    let query = "match $person isa person; not { $person has name $name; };";
    let match_ = typeql::parse_query(query).unwrap().into_structure().into_pipeline().stages.remove(0).into_match();

    // IR
    let empty_function_index = HashMapFunctionSignatureIndex::empty();
    let mut translation_context = PipelineTranslationContext::new();
    let mut value_parameters = ParameterRegistry::new();
    let builder =
        translate_match(&mut translation_context, &mut value_parameters, &empty_function_index, &match_).unwrap();
    let block = builder.finish().unwrap();

    // Executor
    let snapshot = Arc::new(storage.clone().open_snapshot_read());
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let entry_annotations = infer_types(
        &*snapshot,
        &block,
        &translation_context.variable_registry,
        &type_manager,
        &BTreeMap::new(),
        &EmptyAnnotatedFunctionSignatures,
        false,
    )
    .unwrap();

    let conjunction_executable = compiler::executable::match_::planner::compile(
        &block,
        &BTreeMap::new(),
        &HashMap::new(),
        &block.conjunction().named_producible_variables(block.block_context()).collect(),
        &entry_annotations,
        &translation_context.variable_registry,
        &HashMap::new(),
        &statistics,
        &ExecutableFunctionRegistry::empty(),
    )
    .unwrap();
    let executor = ConjunctionExecutor::new(
        &conjunction_executable,
        &snapshot,
        &thing_manager,
        MaybeOwnedRow::empty(),
        Arc::new(ExecutableFunctionRegistry::empty()),
        &QueryProfile::new(false),
    )
    .unwrap();

    let context = ExecutionContext::new(snapshot, thing_manager, Arc::default());
    let iterator = executor.into_iterator(context, ExecutionInterrupt::new_uninterruptible());

    let rows = iterator
        .map_static(|row| row.map(|row| row.into_owned()).map_err(|err| err.clone()))
        .into_iter()
        .unique_by(|res| res.as_ref().unwrap().row().to_vec())
        .try_collect::<_, Vec<_>, _>()
        .unwrap();

    for row in &rows {
        for value in row {
            print!("{}, ", value);
        }
        println!()
    }

    assert_eq!(rows.len(), 1);
}

#[test]
fn test_forall_planning_traversal() {
    let (_tmp_dir, mut storage) = create_core_storage();
    setup_concept_storage(&mut storage);
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let schema = "define
        relation set-membership, relates set, relates item;
        entity set, plays set-membership:set;
        entity item, plays set-membership:item;
    ";
    let data = "insert
        $a isa item; $b isa item; $c isa item;
        $a_ isa set;
        (set: $a_, item: $a) isa set-membership;
        $ab isa set;
        (set: $ab, item: $a) isa set-membership;
        (set: $ab, item: $b) isa set-membership;
        $ac isa set;
        (set: $ac, item: $a) isa set-membership;
        (set: $ac, item: $c) isa set-membership;
        $abc isa set;
        (set: $abc, item: $a) isa set-membership;
        (set: $abc, item: $b) isa set-membership;
        (set: $abc, item: $c) isa set-membership;
    ";

    let statistics = setup(&storage, type_manager, thing_manager, schema, data);

    let query = "match 
        $sup isa set;
        $sub isa set;

        (item: $unique, set: $sup) isa set-membership;
        not { (item: $unique, set: $sub) isa set-membership; };

        not {
            (item: $element, set: $sub) isa set-membership;
            not { (item: $element, set: $sup) isa set-membership; };
        };
    ";
    let match_ = typeql::parse_query(query).unwrap().into_structure().into_pipeline().stages.remove(0).into_match();

    // IR
    let empty_function_index = HashMapFunctionSignatureIndex::empty();
    let mut translation_context = PipelineTranslationContext::new();
    let mut value_parameters = ParameterRegistry::new();
    let builder =
        translate_match(&mut translation_context, &mut value_parameters, &empty_function_index, &match_).unwrap();
    let block = builder.finish().unwrap();

    // Executor
    let snapshot = Arc::new(storage.clone().open_snapshot_read());
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let entry_annotations = infer_types(
        &*snapshot,
        &block,
        &translation_context.variable_registry,
        &type_manager,
        &BTreeMap::new(),
        &EmptyAnnotatedFunctionSignatures,
        false,
    )
    .unwrap();

    let conjunction_executable = compiler::executable::match_::planner::compile(
        &block,
        &BTreeMap::new(),
        &HashMap::new(),
        &block.conjunction().named_producible_variables(block.block_context()).collect(),
        &entry_annotations,
        &translation_context.variable_registry,
        &HashMap::new(),
        &statistics,
        &ExecutableFunctionRegistry::empty(),
    )
    .unwrap();

    let executor = ConjunctionExecutor::new(
        &conjunction_executable,
        &snapshot,
        &thing_manager,
        MaybeOwnedRow::empty(),
        Arc::new(ExecutableFunctionRegistry::empty()),
        &QueryProfile::new(false),
    )
    .unwrap();

    let context = ExecutionContext::new(snapshot, thing_manager, Arc::default());
    let iterator = executor.into_iterator(context, ExecutionInterrupt::new_uninterruptible());

    let rows = iterator
        .map_static(|row| row.map(|row| row.into_owned()).map_err(|err| err.clone()))
        .into_iter()
        .unique_by(|res| res.as_ref().unwrap().row().to_vec())
        .try_collect::<_, Vec<_>, _>()
        .unwrap();

    for row in &rows {
        for value in row {
            print!("{}, ", value);
        }
        println!()
    }

    // 1. ab ⊃ a
    // 2. ac ⊃ a
    // 3. abc ⊃ a ($unique = b)
    // 4. abc ⊃ a ($unique = c)
    // 5. abc ⊃ ab
    // 6. abc ⊃ ac
    assert_eq!(rows.len(), 6);
}

#[test]
fn test_named_var_select() {
    let (_tmp_dir, mut storage) = create_core_storage();
    setup_concept_storage(&mut storage);
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let schema = "define
        attribute age value integer;
        attribute name value string;
        entity person owns age @card(0..), owns name @card(0..);
    ";
    let data = "insert
        $_ isa person, has age 12, has name 'John';
        $_ isa person, has age 14;
        $_ isa person, has name 'Leila';
        $_ isa person;
    ";

    let statistics = setup(&storage, type_manager, thing_manager, schema, data);

    let query = "match $person has name $_, has age $_;";
    let match_ = typeql::parse_query(query).unwrap().into_structure().into_pipeline().stages.remove(0).into_match();

    // IR
    let empty_function_index = HashMapFunctionSignatureIndex::empty();
    let mut translation_context = PipelineTranslationContext::new();
    let mut value_parameters = ParameterRegistry::new();
    let builder =
        translate_match(&mut translation_context, &mut value_parameters, &empty_function_index, &match_).unwrap();
    let block = builder.finish().unwrap();

    // Executor
    let snapshot = Arc::new(storage.clone().open_snapshot_read());
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let entry_annotations = infer_types(
        &*snapshot,
        &block,
        &translation_context.variable_registry,
        &type_manager,
        &BTreeMap::new(),
        &EmptyAnnotatedFunctionSignatures,
        false,
    )
    .unwrap();

    let conjunction_executable = compiler::executable::match_::planner::compile(
        &block,
        &BTreeMap::new(),
        &HashMap::new(),
        &block.conjunction().named_producible_variables(block.block_context()).collect(),
        &entry_annotations,
        &translation_context.variable_registry,
        &HashMap::new(),
        &statistics,
        &ExecutableFunctionRegistry::empty(),
    )
    .unwrap();
    let executor = ConjunctionExecutor::new(
        &conjunction_executable,
        &snapshot,
        &thing_manager,
        MaybeOwnedRow::empty(),
        Arc::new(ExecutableFunctionRegistry::empty()),
        &QueryProfile::new(false),
    )
    .unwrap();

    let context = ExecutionContext::new(snapshot, thing_manager, Arc::default());
    let iterator = executor.into_iterator(context, ExecutionInterrupt::new_uninterruptible());

    let rows = iterator
        .map_static(|row| row.map(|row| row.into_owned()).map_err(|err| err.clone()))
        .into_iter()
        .unique_by(|res| res.as_ref().unwrap().row().to_vec())
        .try_collect::<_, Vec<_>, _>()
        .unwrap();

    for row in &rows {
        let mut non_empty_count = 0;
        for value in row {
            non_empty_count += !value.is_empty() as usize;
            print!("{}, ", value);
        }
        println!();
        assert_eq!(non_empty_count, 1, "expected only $person to have value in output row");
    }

    assert_eq!(rows.len(), 1);
}

#[test]
fn test_disjunction_planning_traversal() {
    let (_tmp_dir, mut storage) = create_core_storage();
    setup_concept_storage(&mut storage);
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let schema = "define
        attribute age value integer;
        attribute name value string;
        entity person owns age @card(0..), owns name @card(0..);
    ";
    let data = "insert
        $_ isa person, has age 12, has name 'John';
        $_ isa person, has age 14;
        $_ isa person, has name 'Leila';
        $_ isa person;
    ";

    let statistics = setup(&storage, type_manager, thing_manager, schema, data);

    let query = "match
        $person isa person;
        { $person has name $n; } or { $person has age $a; };
    ";
    let match_ = typeql::parse_query(query).unwrap().into_structure().into_pipeline().stages.remove(0).into_match();

    // IR
    let empty_function_index = HashMapFunctionSignatureIndex::empty();
    let mut translation_context = PipelineTranslationContext::new();
    let mut value_parameters = ParameterRegistry::new();
    let builder =
        translate_match(&mut translation_context, &mut value_parameters, &empty_function_index, &match_).unwrap();
    let block = builder.finish().unwrap();

    // Executor
    let snapshot = Arc::new(storage.clone().open_snapshot_read());
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let entry_annotations = infer_types(
        &*snapshot,
        &block,
        &translation_context.variable_registry,
        &type_manager,
        &BTreeMap::new(),
        &EmptyAnnotatedFunctionSignatures,
        false,
    )
    .unwrap();

    let conjunction_executable = compiler::executable::match_::planner::compile(
        &block,
        &BTreeMap::new(),
        &HashMap::new(),
        &block.conjunction().named_producible_variables(block.block_context()).collect(),
        &entry_annotations,
        &translation_context.variable_registry,
        &HashMap::new(),
        &statistics,
        &ExecutableFunctionRegistry::empty(),
    )
    .unwrap();
    let executor = ConjunctionExecutor::new(
        &conjunction_executable,
        &snapshot,
        &thing_manager,
        MaybeOwnedRow::empty(),
        Arc::new(ExecutableFunctionRegistry::empty()),
        &QueryProfile::new(false),
    )
    .unwrap();

    let context = ExecutionContext::new(snapshot, thing_manager, Arc::default());
    let iterator = executor.into_iterator(context, ExecutionInterrupt::new_uninterruptible());

    let rows = iterator
        .map_static(|row| row.map(|row| row.into_owned()).map_err(|err| err.clone()))
        .into_iter()
        .unique_by(|res| res.as_ref().unwrap().row().to_vec())
        .try_collect::<_, Vec<_>, _>()
        .unwrap();

    for row in &rows {
        for value in row {
            print!("{}, ", value);
        }
        println!()
    }

    assert_eq!(rows.len(), 3);
}

// #[test]
// FIXME
fn test_disjunction_planning_nested_negations() {
    let (_tmp_dir, mut storage) = create_core_storage();
    setup_concept_storage(&mut storage);
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let schema = "define
        attribute age value integer;
        attribute name value string;
        entity person owns age @card(0..), owns name @card(0..);
    ";
    let data = "insert
        $_ isa person, has age 12, has name 'John';
        $_ isa person, has age 14;
        $_ isa person, has name 'Leila';
        $_ isa person;
    ";

    let statistics = setup(&storage, type_manager, thing_manager, schema, data);

    let query = "match
        $person isa person;
        {
            $person has name $_;
            not { $person has age $_; };
        } or {
            $person has age $_;
            not { $person has name $_; };
        };
    ";
    let match_ = typeql::parse_query(query).unwrap().into_structure().into_pipeline().stages.remove(0).into_match();

    // IR
    let empty_function_index = HashMapFunctionSignatureIndex::empty();
    let mut translation_context = PipelineTranslationContext::new();
    let mut value_parameters = ParameterRegistry::new();
    let builder =
        translate_match(&mut translation_context, &mut value_parameters, &empty_function_index, &match_).unwrap();
    let block = builder.finish().unwrap();

    // Executor
    let snapshot = Arc::new(storage.clone().open_snapshot_read());
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let entry_annotations = infer_types(
        &*snapshot,
        &block,
        &translation_context.variable_registry,
        &type_manager,
        &BTreeMap::new(),
        &EmptyAnnotatedFunctionSignatures,
        false,
    )
    .unwrap();

    let conjunction_executable = compiler::executable::match_::planner::compile(
        &block,
        &BTreeMap::new(),
        &HashMap::new(),
        &block.conjunction().named_producible_variables(block.block_context()).collect(),
        &entry_annotations,
        &translation_context.variable_registry,
        &HashMap::new(),
        &statistics,
        &ExecutableFunctionRegistry::empty(),
    )
    .unwrap();
    let executor = ConjunctionExecutor::new(
        &conjunction_executable,
        &snapshot,
        &thing_manager,
        MaybeOwnedRow::empty(),
        Arc::new(ExecutableFunctionRegistry::empty()),
        &QueryProfile::new(false),
    )
    .unwrap();

    let context = ExecutionContext::new(snapshot, thing_manager, Arc::default());
    let iterator = executor.into_iterator(context, ExecutionInterrupt::new_uninterruptible());

    let rows = iterator
        .map_static(|row| row.map(|row| row.into_owned()).map_err(|err| err.clone()))
        .into_iter()
        .unique_by(|res| res.as_ref().unwrap().row().to_vec())
        .try_collect::<_, Vec<_>, _>()
        .unwrap();

    for row in &rows {
        for value in row {
            print!("{}, ", value);
        }
        println!()
    }

    assert_eq!(rows.len(), 2);
}

#[test]
fn test_mismatched_input_types() {
    let (_tmp_dir, mut storage) = create_core_storage();
    setup_concept_storage(&mut storage);
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);

    let schema = "define
        attribute age value integer;
        attribute name value string;
        relation friendship, relates friend, owns age @card(0..);
        entity person, owns age @card(0..), owns name @card(0..), plays friendship:friend;
    ";
    let data = "insert
        $p1 isa person, has name 'John', has age 25;
        $p2 isa person, has name 'James', has age 27;
        $_ isa friendship, links (friend: $p1, friend: $p2), has age 5;
    ";
    let statistics = setup(&storage, type_manager, thing_manager, schema, data);
    let (type_manager, thing_manager) = load_managers(storage.clone(), None);
    {
        let query = "match
            $x has age $age;
            { $x links (friend: $p); }  or
            { $x has name $n; };
            select $x;
        ";
        let snapshot = Arc::new(storage.clone().open_snapshot_read());
        let conjunction_executable =
            compile_query(&*snapshot, &type_manager, thing_manager.clone(), &statistics, query);
        let executor = ConjunctionExecutor::new(
            &conjunction_executable,
            &snapshot,
            &thing_manager,
            MaybeOwnedRow::empty(),
            Arc::new(ExecutableFunctionRegistry::empty()),
            &QueryProfile::new(false),
        )
        .unwrap();
        let context = ExecutionContext::new(snapshot, thing_manager.clone(), Arc::default());
        let iterator = executor.into_iterator(context, ExecutionInterrupt::new_uninterruptible());
        let rows = iterator
            .map_static(|row| row.map(|row| row.into_owned()).map_err(|err| err.clone()))
            .into_iter()
            .unique_by(|res| res.as_ref().unwrap().row().to_vec())
            .try_collect::<_, Vec<_>, _>()
            .unwrap();

        for row in &rows {
            for value in row {
                print!("{}, ", value);
            }
            println!()
        }
    }

    {
        let query = "match
            { $x isa $_; } or { $_ has $x; };
            select $x;
            distinct;
        ";
        let snapshot = Arc::new(storage.clone().open_snapshot_read());
        let conjunction_executable =
            compile_query(&*snapshot, &type_manager, thing_manager.clone(), &statistics, query);
        let executor = ConjunctionExecutor::new(
            &conjunction_executable,
            &snapshot,
            &thing_manager,
            MaybeOwnedRow::empty(),
            Arc::new(ExecutableFunctionRegistry::empty()),
            &QueryProfile::new(false),
        )
        .unwrap();
        let context = ExecutionContext::new(snapshot, thing_manager.clone(), Arc::default());
        let iterator = executor.into_iterator(context, ExecutionInterrupt::new_uninterruptible());
        let rows = iterator
            .map_static(|row| row.map(|row| row.into_owned()).map_err(|err| err.clone()))
            .into_iter()
            .unique_by(|res| res.as_ref().unwrap().row().to_vec())
            .try_collect::<_, Vec<_>, _>()
            .unwrap();

        for row in &rows {
            for value in row {
                print!("{}, ", value);
            }
            println!()
        }
        debug_assert_ne!(rows.len(), 5); // Returns the 5 attributes if type-inference considers categories.
        debug_assert_eq!(rows.len(), 8);
    }
}

fn compile_query(
    snapshot: &impl ReadableSnapshot,
    type_manager: &TypeManager,
    thing_manager: Arc<ThingManager>,
    statistics: &Statistics,
    query: &str,
) -> ConjunctionExecutable {
    // IR
    let match_ = typeql::parse_query(query).unwrap().into_structure().into_pipeline().stages.remove(0).into_match();
    let empty_function_index = HashMapFunctionSignatureIndex::empty();
    let mut translation_context = PipelineTranslationContext::new();
    let mut value_parameters = ParameterRegistry::new();
    let builder =
        translate_match(&mut translation_context, &mut value_parameters, &empty_function_index, &match_).unwrap();
    let block = builder.finish().unwrap();

    // Executor
    let entry_annotations = infer_types(
        snapshot,
        &block,
        &translation_context.variable_registry,
        &type_manager,
        &BTreeMap::new(),
        &EmptyAnnotatedFunctionSignatures,
        false,
    )
    .unwrap();

    compiler::executable::match_::planner::compile(
        &block,
        &BTreeMap::new(),
        &HashMap::new(),
        &block.conjunction().named_producible_variables(block.block_context()).collect(),
        &entry_annotations,
        &translation_context.variable_registry,
        &HashMap::new(),
        &statistics,
        &ExecutableFunctionRegistry::empty(),
    )
    .unwrap()
}
