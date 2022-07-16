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

bool OpenFlowPrint::preorder(const IR::OF_EqualsMatch* e)  {
    auto reg = e->left->to<IR::OF_Register>();
    if (reg != nullptr && reg->isSlice()) {
        /* field=value/mask */
        visit(e->left);
        buffer += "=";
        if (reg->low) {
            buffer += "${";
            if (e->right->to<IR::OF_Constant>()) {
                visit(e->right);
            } else if (auto value = e->right->to<IR::OF_InterpolatedVarExpression>()) {
                if (reg->is_boolean)
                    buffer += "(if (";
                buffer += value->varname;
                if (reg->is_boolean)
                    buffer += ") 1 else 0)";
            } else {
                BUG("%1%: don't know how to shift left for matching", e->toString());
                visit(e->right);
            }
            buffer += " << " + Util::toString(reg->low) + "}";
        } else {
            visit(e->right);
        }
        buffer += "/" + reg->mask();
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
    size_t i = 0;
    for (auto m : e->matches) {
        if (i++ > 0)
            buffer += ", ";
        visit(m);
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