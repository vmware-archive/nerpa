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

// Implementation of methods for the OF_* IR classes

namespace IR {

const size_t OF_Register::maxRegister = 16;  // maximum register number
const size_t OF_Register::registerSize = 32;  // size of a register in bits
const size_t OF_Register::maxBundleSize = 4;  // xxreg0 has 4 registers, i.e. 128 bits

cstring OF_Register::toString() const {
    return asDDlogString(true);
}

cstring OF_Register::asDDlogString(bool inMatch) const {
    // A register is written differently depending on the position
    // in the OF statement.
    size_t n = number;
    cstring regName = "reg" + Util::toString(n);
    cstring result = "";
    for (size_t i = bundle; i > 1; i >>= 1) {
        result += "x";
        n /= 2;
    }
    result += regName;
    if (inMatch) {
        auto mask = Constant::GetMask(high+1) ^ Constant::GetMask(low);
        result += "/" + Util::toString(mask.value, 0, false, 16);
    } else {
        if (high != registerSize * bundle)
            result += "[" + Util::toString(low);
        if (high > low)
            result += ".." + Util::toString(high);
        result += "]";
    }
    return result;
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
