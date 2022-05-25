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

namespace P4OF {

bool OpenFlowPrint::preorder(const IR::OF_TableMatch* e) {
    buffer += e->toString();
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_Constant* e) {
    buffer += e->toString();
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_Register* e) {
    if (findContext<const IR::OF_Match>()) {
        buffer += e->canonicalName(true);
    } else {
        buffer += e->canonicalName(false);
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
    buffer += e->toString();
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_EqualsMatch* e)  {
    buffer += e->toString();
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_ProtocolMatch* e)  {
    buffer += e->toString();
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_SeqMatch* e)  {
    visit(e->left);
    buffer += ", ";
    visit(e->right);
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
    buffer += e->toString();
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_LoadAction* e)  {
    buffer += e->toString();
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
    buffer += e->toString();
    return false;
}

bool OpenFlowPrint::preorder(const IR::OF_OutputAction* e)  {
    buffer += e->toString();
    return false;
}

}  // namespace P4OF
