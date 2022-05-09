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
#include "frontends/p4/methodInstance.h"
#include "resources.h"

namespace P4OF {

class DLCodeGenerator : public Inspector {
    const OFResources& resources;
    P4::ReferenceMap*    refMap;
    P4::TypeMap*         typeMap;

    IR::Vector<IR::Node> *declarations;
    IR::Vector<IR::Type> *alternatives;
    cstring tableName;
    IR::DDlogProgram* program;

  public:
    explicit DLCodeGenerator(
        const OFResources& resources, P4::ReferenceMap* refMap, P4::TypeMap* typeMap):
            resources(resources), refMap(refMap), typeMap(typeMap), program(nullptr)
    { CHECK_NULL(refMap); CHECK_NULL(typeMap); declarations = new IR::Vector<IR::Node>(); }

    IR::DDlogProgram* getProgram() const { return program; }

    cstring makeId(cstring name) {
        return name.replace(".", "_");
    }

    Visitor::profile_t init_apply(const IR::Node* node) override {
        auto params = new IR::IndexedVector<IR::Parameter>();
        params->push_back(new IR::Parameter(IR::ID("flow"), IR::Direction::None, IR::Type_String::get()));
        declarations->push_back(new IR::DDlogRelation(IR::ID("Flow"), IR::Direction::Out, *params));
        return Inspector::init_apply(node);
    }

    bool preorder(const IR::Type_Typedef* tdef) override {
        auto trans = new IR::DDlogTypedef(tdef->name, tdef->type);
        declarations->push_back(trans);
        return true;
    }

    bool preorder(const IR::P4Table* table) override {
        tableName = makeId(table->externalName());
        alternatives = nullptr;
        return true;
    }

    bool preorder(const IR::Operation_Binary* op) override {
        ::error(ErrorType::ERR_UNSUPPORTED_ON_TARGET,
                "%1%: operation not supported", op);
        return false;
    }

    bool preorder(const IR::ActionListElement* ale) override {
        if (!alternatives)
            alternatives = new IR::Vector<IR::Type>();
        auto mce = ale->expression->to<IR::MethodCallExpression>();
        BUG_CHECK(mce, "%1%: expected a method call", ale->expression);
        auto mi = P4::MethodInstance::resolve(mce, refMap, typeMap);
        auto ac = mi->to<P4::ActionCall>();
        cstring alternative = tableName + "Action" + ac->action->externalName();
        auto fields = new IR::IndexedVector<IR::StructField>();
        BUG_CHECK(mce->arguments->size() == 0, "%1%: expected no arguments", mce);
        for (auto p : ac->action->parameters->parameters) {
            auto field = new IR::StructField(p->srcInfo, p->name, p->type);
            fields->push_back(field);
        }
        auto st = new IR::DDlogTypeStruct(ale->srcInfo, IR::ID(makeId(alternative)), *fields);
        alternatives->push_back(st);
        return false;
    }

    void postorder(const IR::P4Table* table) override {
        cstring typeName = tableName + "Action";
        // Union type representing all possible actions
        auto type = new IR::DDlogTypeAlt(*alternatives);
        auto td = new IR::DDlogTypedef(table->srcInfo, typeName, type);
        declarations->push_back(td);

        auto key = table->getKey();
        CHECK_NULL(key);
        // Parameters of the corresponding P4Runtime relation
        auto params = new IR::IndexedVector<IR::Parameter>();
        // Arguments of a tuple expression
        auto args = new IR::Vector<IR::DDlogExpression>();
        for (auto ke : key->keyElements) {
            auto type = typeMap->getType(ke->expression, true);
            auto match = ke->matchType;
            if (match->path->name.name == "optional") {
                type = new IR::DDlogTypeOption(type);
            }

            auto name = ke->annotations->getSingle(IR::Annotation::nameAnnotation)->getSingleString();
            auto param = new IR::Parameter(ke->srcInfo, name, IR::Direction::None, type);
            params->push_back(param);

            auto varName = new IR::DDlogVarName(name);
            args->push_back(varName);
        }
        params->push_back(new IR::Parameter("priority", IR::Direction::None, IR::Type_Bits::get(32)));
        params->push_back(new IR::Parameter("action", IR::Direction::None, new IR::Type_Name(typeName)));
        auto rel = new IR::DDlogRelation(table->srcInfo, IR::ID(tableName), IR::Direction::In, *params);
        declarations->push_back(rel);

        args->push_back(new IR::DDlogVarName("priority"));
        args->push_back(new IR::DDlogVarName("action"));
        cstring flowRule = "table=";
        size_t id = resources.getTableId(table);
        flowRule += Util::toString(id);
        flowRule += " priority=${priority}";
        auto str = new IR::DDlogStringLiteral(flowRule);
        auto flowTerm = new IR::DDlogAtom(table->srcInfo, "Flow",
                                          new IR::DDlogTupleExpression({str}));
        auto rhs = new IR::Vector<IR::DDlogTerm>();
        auto relationTerm = new IR::DDlogAtom(table->srcInfo, IR::ID(tableName),
                                              new IR::DDlogTupleExpression(*args));
        rhs->push_back(relationTerm);
        auto rule = new IR::DDlogRule(flowTerm, *rhs);
        declarations->push_back(rule);
        tableName = "";
    }

    void end_apply() {
        program = new IR::DDlogProgram(declarations);
    }
};

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

    IR::DDlogProgram* convert() {
        for (auto sf : userMetadataType->fields) {
            resources.allocateRegister(sf);
        }

        ResourceAllocator allocator(resources);
        ingress->apply(allocator);
        egress->apply(allocator);

        DLCodeGenerator gen(resources, refMap, typeMap);
        program->apply(gen);
        return gen.getProgram();
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

    if (options.outputFile.isNullOrEmpty())
        return;
    auto dlStream = openFile(options.outputFile, false);
    if (dlStream == nullptr)
        return;
    ddlogProgram->emit(*dlStream);
}

}  // namespace P4OF
