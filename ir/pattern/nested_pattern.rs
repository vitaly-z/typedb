/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

use std::{collections::HashMap, fmt, mem, ops::ControlFlow};

use answer::variable::Variable;
use structural_equality::StructuralEquality;
use typeql::common::Span;

use crate::{
    pattern::{disjunction::Disjunction, negation::Negation, optional::Optional, VariableBindingMode},
    pipeline::block::BlockContext,
};

#[derive(Debug, Clone)]
pub enum NestedPattern {
    Disjunction(Disjunction),
    Negation(Negation),
    Optional(Optional),
}

impl NestedPattern {
    pub fn as_disjunction(&self) -> Option<&Disjunction> {
        match self {
            NestedPattern::Disjunction(disjunction) => Some(disjunction),
            _ => None,
        }
    }

    pub(crate) fn as_disjunction_mut(&mut self) -> Option<&mut Disjunction> {
        match self {
            NestedPattern::Disjunction(disjunction) => Some(disjunction),
            _ => None,
        }
    }

    pub fn as_negation(&self) -> Option<&Negation> {
        match self {
            NestedPattern::Negation(negation) => Some(negation),
            _ => None,
        }
    }

    pub(crate) fn as_negation_mut(&mut self) -> Option<&mut Negation> {
        match self {
            NestedPattern::Negation(negation) => Some(negation),
            _ => None,
        }
    }

    pub(crate) fn as_optional(&self) -> Option<&Optional> {
        match self {
            NestedPattern::Optional(optional) => Some(optional),
            _ => None,
        }
    }

    pub(crate) fn as_optional_mut(&mut self) -> Option<&mut Optional> {
        match self {
            NestedPattern::Optional(optional) => Some(optional),
            _ => None,
        }
    }

    pub(crate) fn variable_dependency(
        &self,
        block_context: &BlockContext,
    ) -> HashMap<Variable, VariableBindingMode<'_>> {
        match self {
            NestedPattern::Disjunction(disjunction) => disjunction.variable_dependency(block_context),
            NestedPattern::Negation(negation) => negation.variable_dependency(block_context),
            NestedPattern::Optional(optional) => optional.variable_dependency(block_context),
        }
    }

    pub(crate) fn find_disjoint(&self, block_context: &BlockContext) -> ControlFlow<(Variable, Option<Span>)> {
        match self {
            NestedPattern::Disjunction(disjunction) => disjunction.find_disjoint(block_context),
            NestedPattern::Negation(negation) => negation.conjunction().find_disjoint(block_context),
            NestedPattern::Optional(optional) => optional.conjunction().find_disjoint(block_context),
        }
    }
}

impl StructuralEquality for NestedPattern {
    fn hash(&self) -> u64 {
        mem::discriminant(self).hash()
            ^ match self {
                NestedPattern::Disjunction(inner) => inner.hash(),
                NestedPattern::Negation(inner) => inner.hash(),
                NestedPattern::Optional(inner) => inner.hash(),
            }
    }

    fn equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Disjunction(inner), Self::Disjunction(other_inner)) => inner.equals(other_inner),
            (Self::Negation(inner), Self::Negation(other_inner)) => inner.equals(other_inner),
            (Self::Optional(inner), Self::Optional(other_inner)) => inner.equals(other_inner),
            // note: this style forces updating the match when the variants change
            (Self::Disjunction { .. }, _) | (Self::Negation { .. }, _) | (Self::Optional { .. }, _) => false,
        }
    }
}

impl fmt::Display for NestedPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NestedPattern::Disjunction(pattern) => fmt::Display::fmt(pattern, f),
            NestedPattern::Negation(pattern) => fmt::Display::fmt(pattern, f),
            NestedPattern::Optional(pattern) => fmt::Display::fmt(pattern, f),
        }
    }
}
