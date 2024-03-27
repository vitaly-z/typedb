/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

package com.vaticle.typedb.core.graph.adjacency;

import com.vaticle.typedb.core.common.collection.KeyValue;
import com.vaticle.typedb.core.common.iterator.FunctionalIterator;
import com.vaticle.typedb.core.common.iterator.sorted.SortedIterator;
import com.vaticle.typedb.core.common.iterator.sorted.SortedIterator.Forwardable;
import com.vaticle.typedb.core.common.parameters.Order;
import com.vaticle.typedb.core.encoding.Encoding;
import com.vaticle.typedb.core.graph.edge.TypeEdge;
import com.vaticle.typedb.core.graph.vertex.TypeVertex;

public interface TypeAdjacency {

    interface In extends TypeAdjacency {

        InEdgeIterator edge(Encoding.Edge.Type encoding);

        @Override
        default boolean isIn() {
            return true;
        }

        interface InEdgeIterator {

            Forwardable<TypeVertex, Order.Asc> from();

            SortedIterator<TypeVertex, Order.Asc> to();

            FunctionalIterator<TypeVertex> overridden();

            Forwardable<KeyValue<TypeVertex, TypeVertex>, Order.Asc> fromAndOverridden();
        }
    }

    interface Out extends TypeAdjacency {

        OutEdgeIterator edge(Encoding.Edge.Type encoding);

        @Override
        default boolean isOut() {
            return true;
        }

        interface OutEdgeIterator {

            SortedIterator<TypeVertex, Order.Asc> from();

            Forwardable<TypeVertex, Order.Asc> to();

            FunctionalIterator<TypeVertex> overridden();

            Forwardable<KeyValue<TypeVertex, TypeVertex>, Order.Asc> toAndOverridden();
        }
    }

    default boolean isIn() {
        return false;
    }

    default boolean isOut() {
        return false;
    }

    /**
     * Returns an edge of type {@code encoding} that connects to an {@code adjacent}
     * vertex.
     *
     * @param encoding type of the edge to filter by
     * @param adjacent vertex that the edge connects to
     * @return an edge of type {@code encoding} that connects to {@code adjacent}.
     */
    TypeEdge edge(Encoding.Edge.Type encoding, TypeVertex adjacent);

    TypeEdge put(Encoding.Edge.Type encoding, TypeVertex adjacent);

    /**
     * Deletes all edges with a given encoding from the {@code Adjacency} map.
     *
     * This is a recursive delete operation. Deleting the edges from this
     * {@code Adjacency} map will also delete it from the {@code Adjacency} map
     * of the previously adjacent vertex.
     *
     * @param encoding type of the edge to the adjacent vertex
     */
    void delete(Encoding.Edge.Type encoding);

    void deleteAll();

    TypeEdge cache(TypeEdge edge);

    void remove(TypeEdge edge);

    void commit();
}
