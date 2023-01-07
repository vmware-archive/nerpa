/*
Copyright 2022 Vmware, Inc.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

#include "ir/ir.h"
#include "lib/cstring.h"
#include "lib/stringify.h"
#include "lib/algorithm.h"

// Implementation of methods for the OF_* IR classes

namespace IR {

const size_t OF_Register::maxRegister = 16;  // maximum register number
const size_t OF_Register::registerSize = 32;  // size of a register in bits
const size_t OF_Register::maxBundleSize = 4;  // xxreg0 has 4 registers, i.e. 128 bits

cstring OF_Register::toString() const {
    return asDDlogString(true);
}

Constant OF_Register::mask() const {
    return Constant::GetMask(high+1) ^ Constant::GetMask(low);
}

void OF_Register::validate() const {
    BUG_CHECK(low <= high, "low %1% > high %2%", low, high);
    BUG_CHECK(high - low <= size, "high %1% - low %2% > size %3%", high, low, size);
    BUG_CHECK(size <= registerSize * maxBundleSize, "size %1% > max %2%",
              size, registerSize * maxBundleSize);
    size_t bundle = 32;
    size_t bytes = ROUNDUP(size, 8);
    while (bytes > 4) {
        bytes = bytes >> 1;
        bundle = bundle << 1;
    }
    BUG_CHECK(low / bundle == high / bundle,
              "start %1% and end %2% bytes in different registers",
              ROUNDUP(low, 8), ROUNDUP(high, 8));
}

cstring OF_Register::asDDlogString(bool inMatch) const {
    // A register is written differently depending on the position
    // in the OF statement.
    if (!isSlice())
        return name;

    cstring result = name;
    if (!inMatch) {
        if (high != size)
            result += "[" + Util::toString(low);
        if (high > low)
            result += ".." + Util::toString(high);
        result += "]";
    }
    return result;
}

// The 'n' least-significant bits of the register.
const OF_Register* OF_Register::lowBits(size_t n) const {
    BUG_CHECK(n <= width(), "n %1% < width %2", n, width());
    BUG_CHECK(n > 0, "n == 0");
    return new IR::OF_Register(name, size, low, low + n - 1, is_boolean);
}

// The 'n' most-significant bits of the register.
const OF_Register* OF_Register::highBits(size_t n) const {
    BUG_CHECK(n <= width(), "n %1% < width %2", n, width());
    return new IR::OF_Register(name, size, low + (width() - n), high, is_boolean);
}

cstring OF_ResubmitAction::toString() const {
    return cstring("resubmit(,") + Util::toString(nextTable) + ")";
}

cstring OF_Constant::toString() const {
    bool isSigned = false;
    if (auto tb = value->type->to<IR::Type_Bits>())
        isSigned = tb->isSigned;
    return Util::toString(value->value, 0, isSigned, value->base);
}

cstring OF_TableMatch::toString() const {
    return "table=" + Util::toString(id);
}

cstring OF_ProtocolMatch::toString() const {
    return proto + ",";
}

cstring OF_EqualsMatch::toString() const {
    return left->toString() + "=" + right->toString();
}

cstring OF_PriorityMatch::toString() const {
    return "priority=" + priority->toString();
}

cstring OF_Slice::toString() const {
    return base->toString() + "[" + Util::toString(low) + ".." + Util::toString(high) + "]";
}

cstring OF_MatchAndAction::toString() const {
    return match->toString() + " actions=" + action->toString();
}

cstring OF_MoveAction::toString() const {
    return "move(" + src->toString() + "->" + dest->toString() + ")";
}

cstring OF_LoadAction::toString() const {
    return "load(" + src->toString() + "->" + dest->toString() + ")";
}

cstring OF_SeqAction::toString() const {
    if (left->is<IR::OF_EmptyAction>())
        return right->toString();
    if (right->is<IR::OF_EmptyAction>())
        return left->toString();
    return left->toString() + ", " + right->toString();
}

}   // namespace IR
