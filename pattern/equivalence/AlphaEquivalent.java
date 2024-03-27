/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

package com.vaticle.typedb.core.pattern.equivalence;

import com.vaticle.typedb.core.common.iterator.FunctionalIterator;

public interface AlphaEquivalent<T extends AlphaEquivalent<T>> {

    FunctionalIterator<AlphaEquivalence> alphaEquals(T that);
}
