/*
 * Copyright (C) 2020 Grakn Labs
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 *
 */

package grakn.core.common.concurrent;

import grakn.common.collection.Either;
import grakn.core.common.exception.GraknException;

import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicInteger;

import static grakn.core.common.exception.ErrorMessage.Internal.OUT_OF_BOUNDS;

public class ResizingBlockingQueue<E> {

    private final int CAPACITY_INITIAL = 16;
    private final int CAPACITY_MULTIPLIER = 4;
    private final AtomicInteger publishers;
    private final AtomicBoolean needsResizing;
    private ManagedBlockingQueue<Either<E, Done>> queue;
    private final AtomicInteger capacity;

    static class Done {}

    public ResizingBlockingQueue() {
        queue = new ManagedBlockingQueue<>(CAPACITY_INITIAL);
        publishers = new AtomicInteger(0);
        needsResizing = new AtomicBoolean(false);
        capacity = new AtomicInteger(CAPACITY_INITIAL);
    }

    public void incrementPublisher() {
        publishers.incrementAndGet();
    }

    public void decrementPublisher() {
        if (publishers.decrementAndGet() < 0) throw GraknException.of(OUT_OF_BOUNDS);
        if (publishers.compareAndSet(0, -1)) {
            try {
                queue.put(Either.second(new Done()));
            } catch (InterruptedException e) {
                throw GraknException.of(e);
            }
        }
    }

    public E take() {
        try {
            Either<E, Done> result = queue.poll();
            if (result != null) {
                if (result.isFirst()) return result.first();
                else return null;
            } else if (needsResizing.compareAndSet(true, false)) {
                resize();
            }
            result = queue.take();
            if (result.isFirst()) return result.first();
            else return null;
        } catch (InterruptedException e) {
            throw GraknException.of(e);
        }
    }

    private void resize() {
        ManagedBlockingQueue<Either<E, Done>> oldQueue = queue;
        capacity.updateAndGet(oldValue -> oldValue * CAPACITY_MULTIPLIER);
        queue = new ManagedBlockingQueue<>(capacity.get());
        oldQueue.drainTo(queue);
    }

    public void put(final E item) {
        try {
            queue.put(Either.first(item));
            if (queue.remainingCapacity() == 0) needsResizing.set(true);
        } catch (InterruptedException e) {
            throw GraknException.of(e);
        }
    }
}
