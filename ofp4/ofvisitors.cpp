/*
Copyright 2022 VMware, Inc.

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

#include <map>

#include "ofvisitors.h"
#include "ir/ir.h"

namespace OFP4 {

bool OpenFlowPrint::preorder(const IR::OF_TableMatch* e) {
    buffer += e->toString();
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_Constant* e) {
    buffer += e->toString();
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_Register* e) {
    bool inMatch = findContext<const IR::OF_Match>();
    if (!e->friendlyName.isNullOrEmpty()) {
        buffer += "${r_" + e->friendlyName + "(" + Util::toString(inMatch) + ")}";
    } else {
        buffer += e->asDDlogString(inMatch);
    }
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_InterpolatedVarExpression* e) {
    buffer += e->toString();
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_Fieldname* e)  {
    buffer += e->toString();
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_Slice* e)  {
    bool inMatch = findContext<const IR::OF_Match>();
    if (inMatch) {
        visit(e->base);
        auto mask = IR::Constant::GetMask(e->high) ^ IR::Constant::GetMask(e->low);
        buffer += "/" + Util::toString(mask.value, 0, false, 16);
    } else {
        visit(e->base);
        buffer += "[" + Util::toString(e->low) + ".." + Util::toString(e->high) + "]";
    }
    return false;
}

// 'erms' must have at least one element.  All its elements must have 'left'
// that are disjoint slices of the same OF_Register.
static void printRegisterMatch(std::vector<const IR::OF_EqualsMatch*>& erms,
                               OpenFlowPrint& ofp,
                               std::string& buffer)  {
    auto reg0 = erms[0]->left->checkedTo<IR::OF_Register>();

    /* field=value/mask */
    if (erms.size() > 1 || reg0->friendlyName.isNullOrEmpty()) {
        buffer += reg0->name;
    } else {
        buffer += "${r_" + reg0->friendlyName + "(true)}";
    }
    buffer += "=";
    IR::Constant mask = 0;
    if (erms.size() == 1 && !reg0->low) {
        ofp.visit(erms[0]->right);
        mask = reg0->mask();
    } else {
        buffer += "${";

        size_t n = 0;
        for (auto erm : erms) {
            auto reg = erm->left->checkedTo<IR::OF_Register>();
            if ((mask & reg->mask()).value != 0) {
                /* Masks from different matches overlap.  There are three cases:
                 *
                 *     1. The values are constants and bits in corresponding
                 *        positions are the same. Then the overlap makes no
                 *        difference.  We could handle this here by tracking
                 *        constant bits that overlap and verifying that they
                 *        are the same.
                 *
                 *     2. The values are constants and there is at least one
                 *        difference in the values for corresponding
                 *        positions. Then the overlap means that the flow
                 *        cannot possibly match.  By the time we arrive here to
                 *        print the match, it is too late to handle this
                 *        correctly.
                 *
                 *     3. At least one value is expanded from a variable. The
                 *        value bits might match or might not.  We would have
                 *        to do a dynamic comparison in DDlog code; again, it
                 *        is too late by the time we arrive here to print the
                 *        match.
                 *
                 * The code already here handles case #1 correctly, but not
                 * case #2 or #3, and can't yet distinguish.
                 */
                ::error(ErrorType::ERR_UNSUPPORTED_ON_TARGET,
                        "%1%: overlapping bitwise matches on register not yet implemented", reg);
            }
            mask = mask | reg->mask();

            if (n++ > 0)
                buffer += " | ";

            bool needsParens = erms.size() > 1 && reg->low > 0;
            if (needsParens)
                buffer += "(";

            if (erm->right->to<IR::OF_Constant>()) {
                ofp.visit(erm->right);
            } else if (auto value = erm->right->to<IR::OF_InterpolatedVarExpression>()) {
                if (!reg->is_boolean) {
                    buffer += value->varname;
                    if (erms.size() > 1 || reg->low > 0)
                        buffer += " as bit<" + Util::toString(reg->size) + ">";
                } else {
                    buffer += "(if (" + value->varname + ") 1 else 0)";
                }
            } else {
                BUG("%1%: don't know how to shift left for matching", erm->toString());
            }
            if (reg->low > 0)
                buffer += " << " + Util::toString(reg->low);

            if (needsParens)
                buffer += ")";
        }
        buffer += "}";
    }

    if (mask.value != IR::Constant::GetMask(reg0->size).value) {
        buffer += "/" + Util::toString(mask.value, 0, false, 16);
    }
}

bool OpenFlowPrint::preorder(const IR::OF_EqualsMatch* e)  {
    auto reg = e->left->to<IR::OF_Register>();
    if (reg != nullptr && reg->isSlice()) {
        std::vector<const IR::OF_EqualsMatch*> erms;
        erms.push_back(e);
        printRegisterMatch(erms, *this, buffer);
    } else {
        /* field=value */
        visit(e->left);
        buffer += "=";
        visit(e->right);
    }
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_ProtocolMatch* e)  {
    buffer += e->toString();
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_SeqMatch* e)  {
    // 'erms' might have multiple OF_EqualsMatch expression that match on
    // different slices of the same OF_Register.  We have to emit only a single
    // match expression for any collection of these.  Accumulate a vector of
    // all the OF_EqualsMatch expressions for a particular register to emit
    // later.  Emit other expressions immediately.
    std::map<cstring, std::vector<const IR::OF_EqualsMatch*>> erms;
    size_t n = 0;
    for (auto m : e->matches) {
        if (auto em = m->to<IR::OF_EqualsMatch>()) {
            if (auto r = em->left->to<IR::OF_Register>()) {
                erms.emplace(r->name, 0).first->second.emplace_back(em);
                continue;
            }
        }

        if (n++ > 0)
            buffer += ", ";
        visit(m);
    }

    // Emit all the accumulated register matches.
    for (auto erm : erms) {
        if (n++ > 0)
            buffer += ", ";
        printRegisterMatch(erm.second, *this, buffer);
    }
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_EmptyAction*)  {
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_ExplicitAction* e)  {
    buffer += e->toString();
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_MatchAndAction* e)  {
    visit(e->match);
    buffer += " actions=";
    visit(e->action);
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_MoveAction* e)  {
    buffer += "move(";
    visit(e->src);
    buffer += "->";
    visit(e->dest);
    buffer += ")";
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_LoadAction* e)  {
    buffer += "load(";
    visit(e->src);
    buffer += "->";
    visit(e->dest);
    buffer += ")";
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_ResubmitAction* e)  {
    buffer += e->toString();
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_InterpolatedVariableAction* e)  {
    buffer += e->toString();
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_SeqAction* e)  {
    visit(e->left);
    buffer += ", ";
    visit(e->right);
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_DropAction* e)  {
    buffer += e->toString();
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_CloneAction* e)  {
    buffer += "clone(";
    visit(e->action);
    buffer += ")";
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_OutputAction* e)  {
    buffer += "output(";
    visit(e->dest);
    buffer += ")";
    return false;
}

}  // namespace OFP4
