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

#ifndef _EXTENSIONS_OFP4_OFVISITORS_H_
#define _EXTENSIONS_OFP4_OFVISITORS_H_

#include "ir/ir.h"

namespace OFP4 {

/// Optimize an open-flow program
class OpenFlowSimplify : public Transform {
    bool foundResubmit = false;

 public:
    OpenFlowSimplify() { setName("OpenFlowSimplify"); visitDagOnce = false; }

    const IR::Node* postorder(IR::OF_Slice* slice) override {
        if (auto br = slice->base->to<IR::OF_Register>()) {
            // Convert the slice of a register into a register.  We
            // intentionally drop the register's friendlyName here because
            // a friendlyName always refers to the whole register.
            return new IR::OF_Register(
                br->name, br->size, br->low + slice->low, br->low + slice->high);
        }
        return slice;
    }

    const IR::Node* preorder(IR::OF_SeqAction* sequence) override {
        // Stop at the first "resubmit".  OF allows multiple resubmits,
        // but we never generate them
        visit(sequence->left);
        if (foundResubmit)
            return sequence->left;
        visit(sequence->right);
        // Strip out EmptyAction from a sequence of actions
        if (sequence->left->is<IR::OF_EmptyAction>())
            return sequence->right;
        if (sequence->right->is<IR::OF_EmptyAction>())
            return sequence->left;
        prune();
        return sequence;
    }

    const IR::Node* preorder(IR::OF_ResubmitAction* action) override {
        foundResubmit = true;
        prune();
        return action;
    }
};

/// Convert an open-flow program to a string.
class OpenFlowPrint : public Inspector {
    std::string buffer;

 public:
    OpenFlowPrint() { setName("OpenFlowPrint"); visitDagOnce = false; }

    bool preorder(const IR::OF_TableMatch* e) override;
    bool preorder(const IR::OF_Constant* e) override;
    bool preorder(const IR::OF_Register* e) override;
    bool preorder(const IR::OF_InterpolatedVarExpression* e) override;
    bool preorder(const IR::OF_Fieldname* e) override;
    bool preorder(const IR::OF_Slice* e) override;
    bool preorder(const IR::OF_EqualsMatch* e) override;
    bool preorder(const IR::OF_ProtocolMatch* e) override;
    bool preorder(const IR::OF_SeqMatch* e) override;
    bool preorder(const IR::OF_EmptyAction* e) override;
    bool preorder(const IR::OF_ExplicitAction* e) override;
    bool preorder(const IR::OF_MatchAndAction* e) override;
    bool preorder(const IR::OF_MoveAction* e) override;
    bool preorder(const IR::OF_LoadAction* e) override;
    bool preorder(const IR::OF_ResubmitAction* e) override;
    bool preorder(const IR::OF_InterpolatedVariableAction* e) override;
    bool preorder(const IR::OF_SeqAction* e) override;
    bool preorder(const IR::OF_DropAction* e) override;
    bool preorder(const IR::OF_CloneAction* e) override;
    bool preorder(const IR::OF_OutputAction* e) override;

    cstring getString() { return cstring(buffer); }

    static cstring toString(const IR::Node* node) {
        OpenFlowPrint ofp;
        BUG_CHECK(node->is<IR::IOF_Node>(), "%1%: expected an OF node", node);
        node->apply(ofp);
        return ofp.getString();
    }
};

}  // namespace OFP4

#endif  /* _EXTENSIONS_OFP4_OFVISITORS_H_ */
