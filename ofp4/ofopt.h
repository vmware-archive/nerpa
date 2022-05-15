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

#ifndef _EXTENSIONS_OFP4_OFOPT_H_
#define _EXTENSIONS_OFP4_OFOPT_H_

namespace P4OF {

class OpenFlowSimplify : public Transform {
    bool foundResubmit = false;

 public:
    OpenFlowSimplify() { setName("OpenFlowSimplify"); }

    const IR::Node* postorder(IR::OF_Slice* slice) override {
        if (auto br = slice->base->to<IR::OF_Register>()) {
            // convert the slice of a register into a register
            return new IR::OF_Register(
                br->number, br->low + slice->low, br->low + slice->high, br->bundle);
        }
        return slice;
    }

    const IR::Node* postorder(IR::OF_Action* action) override {
        if (foundResubmit)
            return new IR::OF_EmptyAction();
        return action;
    }

    const IR::Node* postorder(IR::OF_SeqAction* sequence) override {
        if (sequence->left->is<IR::OF_EmptyAction>())
            return sequence->right;
        if (sequence->right->is<IR::OF_EmptyAction>())
            return sequence->left;
        return sequence;
    }

    const IR::Node* postorder(IR::OF_ResubmitAction* action) override {
        if (foundResubmit)
            return new IR::OF_EmptyAction();
        foundResubmit = true;
        return action;
    }
};

}  // namespace P4OF

#endif  /* _EXTENSIONS_OFP4_OFOPT_H_ */
