/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

use std::{
    collections::{hash_map, HashMap},
    fmt,
    ops::ControlFlow,
};

use answer::variable::Variable;
use structural_equality::StructuralEquality;
use typeql::common::Span;

use crate::{
    pattern::{
        conjunction::{Conjunction, ConjunctionBuilder},
        BranchID, Scope, ScopeId, VariableBindingMode,
    },
    pipeline::block::{BlockBuilderContext, BlockContext, ScopeTransparency},
};

#[derive(Clone, Debug, Default)]
pub struct Disjunction {
    conjunctions: Vec<Conjunction>,
    branch_ids: Vec<BranchID>,
}

impl Disjunction {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn conjunctions_by_branch_id(&self) -> impl Iterator<Item = (&BranchID, &Conjunction)> {
        self.branch_ids.iter().zip(self.conjunctions.iter())
    }

    pub fn conjunctions(&self) -> &[Conjunction] {
        &self.conjunctions
    }

    pub fn conjunctions_mut(&mut self) -> &mut [Conjunction] {
        &mut self.conjunctions
    }

    pub fn named_producible_variables(&self, block_context: &BlockContext) -> impl Iterator<Item = Variable> + '_ {
        self.producible_variables(block_context).filter(Variable::is_named)
    }

    fn producible_variables(&self, block_context: &BlockContext) -> impl Iterator<Item = Variable> + '_ {
        self.variable_dependency(block_context).into_iter().filter_map(|(v, dep)| dep.is_producing().then_some(v))
    }

    pub fn referenced_variables(&self) -> impl Iterator<Item = Variable> + '_ {
        self.conjunctions().iter().flat_map(|conjunction| conjunction.referenced_variables())
    }

    pub fn optimise_away_unsatisfiable_branches(&mut self, unsatisfiable: Vec<ScopeId>) {
        let unsatisfiable_branch_ids = self
            .conjunctions
            .iter()
            .zip(self.branch_ids.iter())
            .filter_map(|(conj, branch_id)| unsatisfiable.contains(&conj.scope_id()).then_some(*branch_id))
            .collect::<Vec<_>>();
        self.branch_ids.retain(|branch_id| !unsatisfiable_branch_ids.contains(branch_id));
        self.conjunctions.retain(|conj| !unsatisfiable.contains(&conj.scope_id()))
    }

    pub fn required_inputs(&self, block_context: &BlockContext) -> impl Iterator<Item = Variable> + '_ {
        self.variable_dependency(block_context).into_iter().filter_map(|(v, dep)| dep.is_required().then_some(v))
    }

    pub(crate) fn variable_dependency(
        &self,
        block_context: &BlockContext,
    ) -> HashMap<Variable, VariableBindingMode<'_>> {
        if self.conjunctions.is_empty() {
            return HashMap::new();
        }
        let mut dependencies = self.conjunctions[0].variable_dependency(block_context);
        for branch in &self.conjunctions[1..] {
            let branch_dependencies = branch.variable_dependency(block_context);
            for (var, dependency) in &mut dependencies {
                if !branch_dependencies.contains_key(var) && dependency.is_producing() {
                    dependency.set_referencing()
                }
            }
            for (var, mut dependency) in branch_dependencies {
                let entry = dependencies.entry(var);
                match entry {
                    hash_map::Entry::Occupied(mut entry) => {
                        *entry.get_mut() |= dependency;
                    }
                    hash_map::Entry::Vacant(entry) => {
                        if dependency.is_producing() {
                            dependency.set_referencing();
                        }
                        entry.insert(dependency);
                    }
                }
            }
        }
        dependencies
    }

    pub(crate) fn find_disjoint(&self, block_context: &BlockContext) -> ControlFlow<(Variable, Option<Span>)> {
        for conjunction in &self.conjunctions {
            conjunction.find_disjoint(block_context)?;
        }
        ControlFlow::Continue(())
    }
}

impl StructuralEquality for Disjunction {
    fn hash(&self) -> u64 {
        self.conjunctions().hash()
    }

    fn equals(&self, other: &Self) -> bool {
        self.conjunctions().equals(other.conjunctions())
    }
}

impl fmt::Display for Disjunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_assert!(!self.conjunctions.is_empty());
        write!(f, "{}", self.conjunctions[0])?;
        for i in 1..self.conjunctions.len() {
            write!(f, " or {}", self.conjunctions[i])?;
        }
        Ok(())
    }
}

pub struct DisjunctionBuilder<'cx, 'reg> {
    context: &'cx mut BlockBuilderContext<'reg>,
    disjunction: &'cx mut Disjunction,
    scope_id: ScopeId,
}

impl<'cx, 'reg> DisjunctionBuilder<'cx, 'reg> {
    pub fn new(
        context: &'cx mut BlockBuilderContext<'reg>,
        scope_id: ScopeId,
        disjunction: &'cx mut Disjunction,
    ) -> Self {
        Self { context, disjunction, scope_id }
    }

    pub fn add_conjunction(&mut self) -> ConjunctionBuilder<'_, 'reg> {
        let conj_scope_id = self.context.create_child_scope(self.scope_id, ScopeTransparency::Transparent);
        self.disjunction.conjunctions.push(Conjunction::new(conj_scope_id));
        self.disjunction.branch_ids.push(self.context.next_branch_id());
        ConjunctionBuilder::new(self.context, self.disjunction.conjunctions.last_mut().unwrap())
    }
}
