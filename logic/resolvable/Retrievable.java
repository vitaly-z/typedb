/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

package com.vaticle.typedb.core.logic.resolvable;

import com.vaticle.typedb.core.common.exception.TypeDBException;
import com.vaticle.typedb.core.pattern.Conjunction;
import com.vaticle.typedb.core.pattern.constraint.Constraint;
import com.vaticle.typedb.core.pattern.constraint.thing.ThingConstraint;
import com.vaticle.typedb.core.pattern.constraint.type.TypeConstraint;
import com.vaticle.typedb.core.pattern.constraint.value.ValueConstraint;
import com.vaticle.typedb.core.pattern.variable.Variable;
import com.vaticle.typedb.core.traversal.common.Identifier;

import java.util.HashSet;
import java.util.Set;

import static com.vaticle.typedb.core.common.exception.ErrorMessage.Internal.ILLEGAL_STATE;
import static com.vaticle.typedb.core.common.iterator.Iterators.iterate;

public class Retrievable extends Resolvable<Conjunction> {

    private final Set<Identifier.Variable.Retrievable> retrievableIds;
    private Set<Variable> generating;

    public Retrievable(Conjunction conjunction) {
        super(conjunction);
        this.retrievableIds = pattern().retrieves();
    }

    public static Set<Retrievable> extractFrom(Conjunction conjunction, Set<Concludable> toExclude) {
        return com.vaticle.typedb.core.logic.resolvable.Retrievable.Extractor.of(conjunction, toExclude).extract();
    }

    @Override
    public Set<Variable> generating() {
        if (generating == null) {
            HashSet<Variable> variables = new HashSet<>();
            variables().forEach(v -> v.constraints().forEach(c -> {
                if (c.isValue() && c.asValue().isAssignment()) variables.add(v);
            }));
            generating = variables;
        }
        return generating;
    }

    @Override
    public Set<Identifier.Variable.Retrievable> retrieves() {
        return retrievableIds;
    }

    @Override
    public Set<Variable> variables() { return pattern().variables(); }

    @Override
    public Retrievable asRetrievable() {
        return this;
    }

    @Override
    public boolean isRetrievable() {
        return true;
    }

    public static class Extractor {
        private final Conjunction conjunction;
        private final Set<Concludable> concludables;
        private final Set<SubgraphRegistry> subgraphs = new HashSet<>();
        private final Set<Variable> extractedVariables = new HashSet<>();
        private final Set<Constraint> extractedConstraints = new HashSet<>();

        public Extractor(Conjunction conjunction, Set<Concludable> concludables) {
            this.conjunction = conjunction;
            this.concludables = concludables;
        }

        public static Extractor of(Conjunction conjunction, Set<Concludable> concludables) {
            return new Extractor(conjunction, concludables);
        }

        public Set<Retrievable> extract() {
            concludables.forEach(concludable ->
                                         iterate(concludable.concludableConstraints())
                                                 .filter(constraint ->
                                                                 !(constraint.isType() && constraint.asType().isLabel()
                                                                         && constraint.asType().owner().id().isLabel())
                                                 ).forEachRemaining(extractedConstraints::add)
            );
            iterate(conjunction.variables()).filter(var -> var.id().isRetrievable()).forEachRemaining(var -> {
                if (!extractedVariables.contains(var)) {
                    SubgraphRegistry subgraph = new SubgraphRegistry();
                    subgraph.registerVariable(var);
                    subgraphs.add(subgraph);
                    extractedVariables.addAll(subgraph.registeredVariables());
                    iterate(subgraph.registeredConstraints).filter(constraint ->
                                                                           !(constraint.isType() && constraint.asType().isLabel()
                                                                                   && constraint.asType().owner().id().isLabel())
                    ).forEachRemaining(extractedConstraints::add);
                }
            });
            return iterate(subgraphs).filter(SubgraphRegistry::isValid).map(subgraph -> {
                Set<TypeConstraint> labelConstraints = iterate(subgraph.registeredConstraints)
                        .filter(Constraint::isType).map(Constraint::asType).filter(TypeConstraint::isLabel).toSet();
                Set<? extends Constraint> otherConstraints = new HashSet<>(subgraph.registeredConstraints);
                otherConstraints.removeAll(labelConstraints);
                Conjunction.ConstraintCloner cloner = Conjunction.ConstraintCloner.cloneExactly(labelConstraints, otherConstraints);
                return new Retrievable(cloner.conjunction());
            }).toSet();
        }

        private class SubgraphRegistry {

            private final Set<Variable> registeredVariables = new HashSet<>();
            private final Set<Constraint> registeredConstraints = new HashSet<>();

            Set<Variable> registeredVariables() {
                return registeredVariables;
            }

            private boolean isValid() {
                return registeredConstraints.size() > 0;
            }

            private void registerVariable(Variable variable) {
                if (!registeredVariables.contains(variable)) {
                    registeredVariables.add(variable);
                    if (variable.id().isRetrievable()) registerRetrievable(variable);
                    else registerLabeled(variable);
                }
            }

            private void registerRetrievable(Variable variable) {
                assert variable.id().isRetrievable();
                variable.constraints().forEach(this::registerConstraint);
                variable.constraining().forEach(this::registerConstraint);
            }

            private void registerLabeled(Variable variable) {
                assert variable.id().isLabel() && variable.asType().label().isPresent();
                registeredConstraints.add(variable.asType().label().get());
            }

            private void registerConstraint(Constraint constraint) {
                if (constraint.isThing()) registerConstraint(constraint.asThing());
                else if (constraint.isType()) registerConstraint(constraint.asType());
                else if (constraint.isValue()) registerConstraint(constraint.asValue());
                else throw TypeDBException.of(ILLEGAL_STATE);
            }

            private void registerConstraint(ThingConstraint thingConstraint) {
                if (!extractedConstraints.contains(thingConstraint)) {
                    registeredConstraints.add(thingConstraint);
                    iterate(thingConstraint.variables()).forEachRemaining(this::registerVariable);
                }
            }

            private void registerConstraint(TypeConstraint typeConstraint) {
                if (!extractedConstraints.contains(typeConstraint)) {
                    registeredConstraints.add(typeConstraint);
                    iterate(typeConstraint.variables()).forEachRemaining(this::registerVariable);
                }
            }

            private void registerConstraint(ValueConstraint valueConstraint) {
                if (!extractedConstraints.contains(valueConstraint)) {
                    registeredConstraints.add(valueConstraint);
                    iterate(valueConstraint.variables()).forEachRemaining(this::registerVariable);
                }
            }
        }
    }
}
