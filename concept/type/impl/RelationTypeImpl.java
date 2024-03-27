/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

package com.vaticle.typedb.core.concept.type.impl;

import com.vaticle.typedb.core.common.exception.TypeDBException;
import com.vaticle.typedb.core.common.iterator.FunctionalIterator;
import com.vaticle.typedb.core.common.iterator.sorted.SortedIterator.Forwardable;
import com.vaticle.typedb.core.common.parameters.Concept.Existence;
import com.vaticle.typedb.core.common.parameters.Concept.Transitivity;
import com.vaticle.typedb.core.common.parameters.Order;
import com.vaticle.typedb.core.concept.ConceptManager;
import com.vaticle.typedb.core.concept.thing.Relation;
import com.vaticle.typedb.core.concept.thing.impl.RelationImpl;
import com.vaticle.typedb.core.concept.type.AttributeType;
import com.vaticle.typedb.core.concept.type.RelationType;
import com.vaticle.typedb.core.concept.type.RoleType;
import com.vaticle.typedb.core.concept.validation.DeclarationValidation;
import com.vaticle.typedb.core.concept.validation.SubtypeValidation;
import com.vaticle.typedb.core.graph.edge.TypeEdge;
import com.vaticle.typedb.core.graph.vertex.ThingVertex;
import com.vaticle.typedb.core.graph.vertex.TypeVertex;
import com.vaticle.typeql.lang.common.TypeQLToken;
import com.vaticle.typeql.lang.common.TypeQLToken.Annotation;

import java.util.ArrayList;
import java.util.List;
import java.util.Set;

import static com.vaticle.typedb.core.common.exception.ErrorMessage.TypeRead.TYPE_ROOT_MISMATCH;
import static com.vaticle.typedb.core.common.exception.ErrorMessage.TypeWrite.RELATION_ABSTRACT_ROLE;
import static com.vaticle.typedb.core.common.exception.ErrorMessage.TypeWrite.RELATION_NO_ROLE;
import static com.vaticle.typedb.core.common.exception.ErrorMessage.TypeWrite.ROOT_TYPE_MUTATION;
import static com.vaticle.typedb.core.common.exception.ErrorMessage.TypeWrite.SCHEMA_VALIDATION_INVALID_DEFINE;
import static com.vaticle.typedb.core.common.exception.ErrorMessage.TypeWrite.SCHEMA_VALIDATION_INVALID_SET_SUPERTYPE;
import static com.vaticle.typedb.core.common.exception.ErrorMessage.TypeWrite.SCHEMA_VALIDATION_INVALID_UNDEFINE;
import static com.vaticle.typedb.core.common.exception.ErrorMessage.TypeWrite.TYPE_HAS_INSTANCES_SET_ABSTRACT;
import static com.vaticle.typedb.core.common.iterator.sorted.SortedIterators.Forwardable.iterateSorted;
import static com.vaticle.typedb.core.common.parameters.Concept.Existence.STORED;
import static com.vaticle.typedb.core.common.parameters.Concept.Transitivity.EXPLICIT;
import static com.vaticle.typedb.core.common.parameters.Concept.Transitivity.TRANSITIVE;
import static com.vaticle.typedb.core.common.parameters.Order.Asc.ASC;
import static com.vaticle.typedb.core.encoding.Encoding.Edge.Type.RELATES;
import static com.vaticle.typedb.core.encoding.Encoding.Edge.Type.SUB;
import static com.vaticle.typedb.core.encoding.Encoding.Vertex.Type.RELATION_TYPE;
import static com.vaticle.typedb.core.encoding.Encoding.Vertex.Type.Root.RELATION;
import static com.vaticle.typeql.lang.common.TypeQLToken.Char.COMMA;
import static com.vaticle.typeql.lang.common.TypeQLToken.Char.NEW_LINE;
import static com.vaticle.typeql.lang.common.TypeQLToken.Char.SEMICOLON;
import static com.vaticle.typeql.lang.common.TypeQLToken.Char.SPACE;
import static java.util.Comparator.comparing;

public class RelationTypeImpl extends ThingTypeImpl implements RelationType {

    public RelationTypeImpl(ConceptManager conceptMgr, TypeVertex vertex) {
        super(conceptMgr, vertex);
        if (vertex.encoding() != RELATION_TYPE) {
            throw exception(TypeDBException.of(TYPE_ROOT_MISMATCH, vertex.label(),
                    RELATION_TYPE.root().label(), vertex.encoding().root().label()));
        }
    }

    private RelationTypeImpl(ConceptManager conceptMgr, String label) {
        super(conceptMgr, label, RELATION_TYPE);
    }

    public static RelationType of(ConceptManager conceptMgr, String label) {
        return new RelationTypeImpl(conceptMgr, label);
    }

    @Override
    public void setLabel(String label) {
        vertex.label(label);
        vertex.outs().edge(RELATES).to().forEachRemaining(v -> v.scope(label));
    }

    @Override
    public void setAbstract() {
        if (isAbstract()) return;
        if (getInstances(EXPLICIT).first().isPresent()) {
            throw exception(TypeDBException.of(TYPE_HAS_INSTANCES_SET_ABSTRACT, getLabel()));
        }
        vertex.isAbstract(true);
    }

    @Override
    public void unsetAbstract() {
        vertex.isAbstract(false);
        declaredRoles().forEachRemaining(RoleTypeImpl::unsetAbstract);
    }

    @Override
    public RelationTypeImpl getSupertype() {
        return vertex.outs().edge(SUB).to().map(t -> (RelationTypeImpl) conceptMgr.convertRelationType(t)).firstOrNull();
    }

    @Override
    public Forwardable<RelationTypeImpl, Order.Asc> getSupertypes() {
        return iterateSorted(graphMgr().schema().getSupertypes(vertex), ASC)
                .filter(TypeVertex::isRelationType)
                .mapSorted(t -> (RelationTypeImpl) conceptMgr.convertRelationType(t), t -> t.vertex, ASC);
    }

    @Override
    public void setSupertype(RelationType superType) {
        validateIsNotDeleted();
        SubtypeValidation.collectExceptions(
                SubtypeValidation.Relates.validateSetSupertype(this, superType),
                SubtypeValidation.Plays.validateSetSupertype(this, superType),
                SubtypeValidation.Owns.validateSetSupertype(this, superType)
        ).ifPresent(e -> {
                throw exception(TypeDBException.of(SCHEMA_VALIDATION_INVALID_SET_SUPERTYPE, this.getLabel(), superType.getLabel(), e));
        });
        setSuperTypeVertex(((RelationTypeImpl) superType).vertex);
    }

    @Override
    public Forwardable<RelationTypeImpl, Order.Asc> getSubtypes() {
        return getSubtypes(TRANSITIVE);
    }

    @Override
    public Forwardable<RelationTypeImpl, Order.Asc> getSubtypes(Transitivity transitivity) {
        return getSubtypes(transitivity, v -> (RelationTypeImpl) conceptMgr.convertRelationType(v));
    }

    @Override
    public Forwardable<RelationImpl, Order.Asc> getInstances() {
        return getInstances(TRANSITIVE);
    }

    @Override
    public Forwardable<RelationImpl, Order.Asc> getInstances(Transitivity transitivity) {
        return instances(transitivity, v -> RelationImpl.of(conceptMgr, v));
    }

    @Override
    public void setRelates(String roleLabel) {
        validateIsNotDeleted();
        DeclarationValidation.Relates.validateAdd(this, roleLabel).stream().findFirst().ifPresent(e -> { throw exception(e); });
        SubtypeValidation.collectExceptions(SubtypeValidation.Relates.validateAdd(this, roleLabel)).ifPresent(e -> {
            throw exception(TypeDBException.of(SCHEMA_VALIDATION_INVALID_DEFINE, SubtypeValidation.Relates.format(this.getLabel().toString(), roleLabel, null), e));
        });
        TypeVertex roleTypeVertex = graphMgr().schema().getType(roleLabel, vertex.label());
        RoleTypeImpl roleType;
        if (roleTypeVertex == null) {
            roleType = RoleTypeImpl.of(conceptMgr, roleLabel, vertex.label());
            vertex.outs().put(RELATES, roleType.vertex);
        } else {
            roleType = (RoleTypeImpl) conceptMgr.convertRoleType(roleTypeVertex);
            roleType.setSupertype(conceptMgr.convertRoleType(graphMgr().schema().rootRoleType()));
        }
        assert roleType.getSupertype() != null;
        vertex.outs().edge(RELATES, roleType.vertex).setOverridden(((RoleTypeImpl) roleType.getSupertype()).vertex);
    }

    @Override
    public void setRelates(String roleLabel, String overriddenLabel) {
        validateIsNotDeleted();
        assert getSupertype() != null;
        DeclarationValidation.Relates.validateAdd(this, roleLabel).stream().findFirst().ifPresent(e -> { throw exception(e); });
        DeclarationValidation.Relates.validateOverride(this, roleLabel, overriddenLabel).stream().findFirst().ifPresent(e -> { throw exception(e); });
        RoleTypeImpl overriddenType = (RoleTypeImpl) getSupertype().getRelates(TRANSITIVE, overriddenLabel);
        SubtypeValidation.collectExceptions(
            SubtypeValidation.Relates.validateAdd(this, roleLabel),
            SubtypeValidation.Relates.validateOverride(this, overriddenType)
        ).ifPresent(e -> {
            throw exception(TypeDBException.of(SCHEMA_VALIDATION_INVALID_DEFINE, SubtypeValidation.Relates.format(this.getLabel().toString(), roleLabel, overriddenLabel), e));
        });

        setRelates(roleLabel);
        RoleTypeImpl roleType = (RoleTypeImpl) this.getRelates(roleLabel);
        roleType.setSupertype(overriddenType);
        vertex.outs().edge(RELATES, roleType.vertex).setOverridden(overriddenType.vertex);
    }

    @Override
    public void unsetRelates(String roleLabel) {
        validateIsNotDeleted();
        RoleType roleType = getRelates(roleLabel);
        SubtypeValidation.collectExceptions(SubtypeValidation.Relates.validateRemove(this, roleType)).ifPresent(e -> {
            throw exception(TypeDBException.of(SCHEMA_VALIDATION_INVALID_UNDEFINE,
                    SubtypeValidation.Relates.format(this, roleType, getRelatesOverridden(roleType.getLabel().toString())), e));
        });
        roleType.delete();
    }

    @Override
    public Forwardable<RoleTypeImpl, Order.Asc> getRelates() {
        return getRelates(TRANSITIVE);
    }

    @Override
    public Forwardable<RoleTypeImpl, Order.Asc> getRelates(Transitivity transitivity) {
        return getRelatesVertices(transitivity).mapSorted(v -> (RoleTypeImpl) conceptMgr.convertRoleType(v), roleType -> roleType.vertex, ASC);
    }

    Forwardable<TypeVertex, Order.Asc> getRelatesVertices(Transitivity transitivity) {
        if (transitivity == EXPLICIT) return vertex.outs().edge(RELATES).to();
        else return iterateSorted(graphMgr().schema().relatedRoleTypes(vertex), ASC);
    }

    @Override
    public RoleType getRelatesOverridden(String roleLabel) {
        TypeVertex roleVertex = graphMgr().schema().getType(roleLabel, vertex.label());
        if (roleVertex != null) {
            TypeEdge relatesEdge = vertex.outs().edge(RELATES, roleVertex);
            if (relatesEdge != null &&
                    relatesEdge.overridden().isPresent() &&
                    !relatesEdge.overridden().get().equals(graphMgr().schema().rootRoleType()))
                return conceptMgr.convertRoleType(relatesEdge.overridden().get());
        }
        return null;
    }

    private FunctionalIterator<RoleTypeImpl> declaredRoles() {
        return vertex.outs().edge(RELATES).to().map(v -> (RoleTypeImpl) conceptMgr.convertRoleType(v));
    }

    @Override
    public RoleType getRelates(String roleLabel) {
        return getRelates(TRANSITIVE, roleLabel);
    }

    /**
     * Get the role type with a given {@code roleLabel} related by this relation type.
     * <p>
     * First, look up the role type by the given label and it's scope: the relation label.
     * If the role type vertex do not exist, then call {@code role()} to get the inherited role types,
     * and see if the any of them has the {@code roleLabel} of interest.
     *
     * @param roleLabel the label of the role
     * @return the role type related in this relation
     */
    @Override
    public RoleType getRelates(Transitivity transitivity, String roleLabel) {
        TypeVertex roleTypeVertex = graphMgr().schema().getType(roleLabel, vertex.label());
        if (roleTypeVertex != null) return conceptMgr.convertRoleType(roleTypeVertex);
        else if (transitivity == TRANSITIVE) return getRelates().filter(role -> role.getLabel().name().equals(roleLabel)).first().orElse(null);
        else return null;
    }

    @Override
    public void delete() {
        validateDelete();
        declaredRoles().forEachRemaining(RoleTypeImpl::delete);
        vertex.delete();
    }

    @Override
    public List<TypeDBException> exceptions() {
        List<TypeDBException> exceptions = new ArrayList<>(super.exceptions());
        if (!isRoot() && !isAbstract() && !getRelates().filter(rt -> !rt.isRoot()).hasNext()) {
            exceptions.add(TypeDBException.of(RELATION_NO_ROLE, this.getLabel()));
        } else if (!isAbstract()) getRelates().filter(rt -> !rt.isRoot() && rt.isAbstract()).forEachRemaining(
                rt -> exceptions.add(TypeDBException.of(RELATION_ABSTRACT_ROLE, getLabel(), rt.getLabel()))
        );
        return exceptions;
    }

    @Override
    public RelationImpl create() {
        return create(STORED);
    }

    @Override
    public RelationImpl create(Existence existence) {
        validateCanHaveInstances(Relation.class);
        ThingVertex.Write instance = graphMgr().data().create(vertex, existence);
        return RelationImpl.of(conceptMgr, instance);
    }

    @Override
    public boolean isRelationType() {
        return true;
    }

    @Override
    public RelationTypeImpl asRelationType() {
        return this;
    }

    @Override
    public void getSyntax(StringBuilder builder) {
        writeSupertype(builder);
        writeAbstract(builder);
        writeOwns(builder);
        writeRelates(builder);
        writePlays(builder);
        builder.append(SEMICOLON).append(NEW_LINE);
    }

    private void writeRelates(StringBuilder builder) {
        getRelates(EXPLICIT).stream().sorted(comparing(x -> x.getLabel().name())).forEach(roleType -> {
            builder.append(COMMA).append(SPACE)
                    .append(TypeQLToken.Constraint.RELATES).append(SPACE)
                    .append(roleType.getLabel().name());
            RoleType overridden = getRelatesOverridden(roleType.getLabel().name());
            if (overridden != null) {
                builder.append(SPACE).append(TypeQLToken.Constraint.AS).append(SPACE)
                        .append(overridden.getLabel().name());
            }
        });
    }

    public static class Root extends RelationTypeImpl {

        public Root(ConceptManager conceptMgr, TypeVertex vertex) {
            super(conceptMgr, vertex);
            assert vertex.label().equals(RELATION.label());
        }

        @Override
        public boolean isRoot() {
            return true;
        }

        @Override
        public void delete() {
            throw exception(TypeDBException.of(ROOT_TYPE_MUTATION));
        }

        @Override
        public void setLabel(String label) {
            throw exception(TypeDBException.of(ROOT_TYPE_MUTATION));
        }

        @Override
        public void unsetAbstract() {
            throw exception(TypeDBException.of(ROOT_TYPE_MUTATION));
        }

        @Override
        public RelationTypeImpl getSupertype() {
            return null;
        }

        @Override
        public Forwardable<RelationTypeImpl, Order.Asc> getSupertypes() {
            return iterateSorted(ASC, this);
        }

        @Override
        public void setSupertype(RelationType superType) {
            throw exception(TypeDBException.of(ROOT_TYPE_MUTATION));
        }

        @Override
        public void setRelates(String roleLabel) {
            throw exception(TypeDBException.of(ROOT_TYPE_MUTATION));
        }

        @Override
        public void setRelates(String roleLabel, String overriddenLabel) {
            throw exception(TypeDBException.of(ROOT_TYPE_MUTATION));
        }

        @Override
        public void unsetRelates(String roleLabel) {
            throw exception(TypeDBException.of(ROOT_TYPE_MUTATION));
        }

        @Override
        public void setOwns(AttributeType attributeType, Set<Annotation> annotations) {
            throw exception(TypeDBException.of(ROOT_TYPE_MUTATION));
        }

        @Override
        public void setOwns(AttributeType attributeType, AttributeType overriddenType, Set<Annotation> annotations) {
            throw exception(TypeDBException.of(ROOT_TYPE_MUTATION));
        }

        @Override
        public void setPlays(RoleType roleType) {
            throw exception(TypeDBException.of(ROOT_TYPE_MUTATION));
        }

        @Override
        public void setPlays(RoleType roleType, RoleType overriddenType) {
            throw exception(TypeDBException.of(ROOT_TYPE_MUTATION));
        }

        @Override
        public void unsetPlays(RoleType roleType) {
            throw exception(TypeDBException.of(ROOT_TYPE_MUTATION));
        }
    }
}
