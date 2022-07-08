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

#ifndef _EXTENSIONS_OFP4_LOWER_H_
#define _EXTENSIONS_OFP4_LOWER_H_

#include "ir/ir.h"
#include "frontends/p4/typeChecking/typeChecker.h"
#include "frontends/common/resolveReferences/resolveReferences.h"

namespace OFP4 {

/**
  This pass rewrites expressions which are not supported natively on OFP4.
*/
class LowerExpressions : public Transform {
    P4::ReferenceMap* refMap;
    P4::TypeMap* typeMap;
    const IR::PathExpression* createTemporary(const IR::Expression* expression);
    IR::IndexedVector<IR::Declaration> newDecls;
    IR::IndexedVector<IR::StatOrDecl>  assignments;

 public:
    LowerExpressions(P4::ReferenceMap* refMap, P4::TypeMap* typeMap) :
            refMap(refMap), typeMap(typeMap)
    { CHECK_NULL(refMap); CHECK_NULL(typeMap); setName("LowerExpressions"); }

    const IR::Node* postorder(IR::Expression* expression) override;
    const IR::Node* postorder(IR::Operation_Relation* expression) override;
    const IR::Node* postorder(IR::LNot* expression) override;
    const IR::Node* postorder(IR::Statement* statement) override;
    const IR::Node* postorder(IR::P4Control* control) override;
};

/**
   Convert a = bexp;
   for 'bexp' a boolean expression
   into
   if (bexp) a = true; else a = false;
*/
class RemoveBooleanValues : public Transform {
    P4::ReferenceMap* refMap;
    P4::TypeMap* typeMap;

 public:
    RemoveBooleanValues(P4::ReferenceMap* refMap, P4::TypeMap* typeMap) :
            refMap(refMap), typeMap(typeMap)
    { CHECK_NULL(refMap); CHECK_NULL(typeMap); setName("RemoveBooleanValues"); }

    const IR::Node* postorder(IR::AssignmentStatement* statement) override;
};

class Lower : public PassRepeated {
 public:
    Lower(P4::ReferenceMap* refMap, P4::TypeMap* typeMap) {
        setName("Lower");
        passes.push_back(new P4::TypeChecking(refMap, typeMap));
        passes.push_back(new RemoveBooleanValues(refMap, typeMap));
        passes.push_back(new P4::TypeChecking(refMap, typeMap));
        passes.push_back(new LowerExpressions(refMap, typeMap));
    }
};

}  // namespace OFP4

#endif  /* _EXTENSIONS_OFP4_LOWER_H_ */
