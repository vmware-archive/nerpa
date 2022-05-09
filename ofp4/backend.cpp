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

#include <vector>
#include <map>

#include "backend.h"
#include "ir/ir.h"
#include "lib/sourceCodeBuilder.h"
#include "lib/nullstream.h"
#include "frontends/p4/evaluator/evaluator.h"
#include "resources.h"

namespace P4OF {

/// Class representing a DDlog program.
class DDlogProgram {
 public:
    DDlogProgram() = default;
    void emit(cstring file);
};

void DDlogProgram::emit(cstring file) {
    auto dlStream = openFile(file, false);
    if (dlStream != nullptr) {
        // TODO
        ;
    }
    dlStream->flush();
}

class ResourceAllocator : public Inspector {
    OFResources& resources;
 public:
    explicit ResourceAllocator(OFResources& resources): resources(resources) {}
    bool preorder(const IR::P4Table* table) {
        resources.allocateTable(table);
        return false;
    }
    bool preorder(const IR::Declaration_Variable* decl) {
        resources.allocateRegister(decl);
        return false;
    }
};

class P4OFProgram {
    const IR::P4Program* program;
    const IR::ToplevelBlock* top;
    P4::ReferenceMap*    refMap;
    P4::TypeMap*         typeMap;
    const IR::P4Control* ingress;
    const IR::P4Control* egress;
    const IR::Type_Struct* headersType;
    const IR::Type_Struct* standardMetadataType;
    const IR::Type_Struct* userMetadataType;
    OFResources resources;

 public:
    P4OFProgram(const IR::P4Program* program, const IR::ToplevelBlock* top,
                P4::ReferenceMap* refMap, P4::TypeMap* typeMap):
            program(program), top(top), refMap(refMap), typeMap(typeMap), resources(typeMap) {
        CHECK_NULL(refMap); CHECK_NULL(typeMap); CHECK_NULL(top); CHECK_NULL(program);
    }

    void build() {
        auto pack = top->getMain();
        CHECK_NULL(pack);
        if (pack->type->name != "OfSwitch")
            ::warning(ErrorType::WARN_INVALID, "%1%: the main package should be called OfSwitch"
                      "; are you using the wrong architecture?", pack->type->name);
        if (pack->getConstructorParameters()->size() != 2) {
            ::error(ErrorType::ERR_MODEL,
                "Expected toplevel package %1% to have 2 parameters", pack->type);
            return;
        }

        auto ig = pack->getParameterValue("ig")->checkedTo<IR::ControlBlock>();
        if (!ig)
            ::error(ErrorType::ERR_MODEL, "No parameter named 'ig' for OfSwitch package.");
        ingress = ig->container;

        auto params = ingress->type->applyParams;
        if (params->size() != 3) {
            ::error(ErrorType::ERR_EXPECTED,
                    "Expected ingress block to have exactly 3 parameters");
            return;
        }

        auto eg = pack->getParameterValue("eg")->checkedTo<IR::ControlBlock>();
        if (!eg)
            ::error(ErrorType::ERR_MODEL, "No parameter named 'eg' for OfSwitch package.");
        egress = eg->container;

        params = egress->type->applyParams;
        if (params->size() != 3) {
            ::error(ErrorType::ERR_EXPECTED,
                    "Expected egress block to have exactly 3 parameters");
            return;
        }

        auto it = params->parameters.begin();
        auto headerParam = *it; ++it;
        auto userMetaParam = *it; ++it;
        auto metaParam = *it;

        auto ht = typeMap->getType(headerParam);
        if (ht == nullptr)
            return;
        headersType = ht->to<IR::Type_Struct>();  // a struct full of headers
        if (!headersType)
            ::error(ErrorType::ERR_MODEL,
                    "%1%: expected a struct type, not %2%", headerParam, ht);

        auto mt = typeMap->getType(metaParam);
        if (mt == nullptr)
            return;
        standardMetadataType = mt->to<IR::Type_Struct>();
        if (!standardMetadataType)
            ::error(ErrorType::ERR_MODEL,
                    "%1%: expected a struct type, not %2%", metaParam, mt);

        auto umt = typeMap->getType(userMetaParam);
        if (umt == nullptr)
            return;
        userMetadataType = umt->to<IR::Type_Struct>();
        if (!userMetadataType)
            ::error(ErrorType::ERR_MODEL,
                    "%1%: expected a struct type, not %2%", userMetadataType, umt);
    }

    DDlogProgram* convert() {
        for (auto sf : userMetadataType->fields) {
            resources.allocateRegister(sf);
        }

        ResourceAllocator allocator(resources);
        ingress->apply(allocator);
        egress->apply(allocator);
        return nullptr;
    }
};

void BackEnd::run(P4OFOptions& options, const IR::P4Program* program) {
    P4::EvaluatorPass evaluator(refMap, typeMap);
    program = program->apply(evaluator);
    if (::errorCount() > 0)
        return;
    auto top = evaluator.getToplevelBlock();
    auto main = top->getMain();
    if (main == nullptr) {
        ::warning(ErrorType::WARN_MISSING,
                  "Could not locate top-level block; is there a '%1%' package?",
                  IR::P4Program::main);
        return;
    }
    P4OFProgram ofp(program, top, refMap, typeMap);
    ofp.build();
    if (::errorCount() > 0)
        return;
    auto ddlogProgram = ofp.convert();
    if (!ddlogProgram)
        return;
    ddlogProgram->emit(options.outputFile);
}

}  // namespace P4OF
