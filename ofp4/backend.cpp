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
#include "ofopt.h"
#include "ir/ir.h"
#include "lib/sourceCodeBuilder.h"
#include "lib/nullstream.h"
#include "frontends/p4/evaluator/evaluator.h"
#include "frontends/p4/methodInstance.h"
#include "frontends/p4/evaluator/substituteParameters.h"
#include "resources.h"

namespace P4OF {

/// Can be used to translate action bodies or expressions into OF actions/expressions
class ActionTranslator : public Inspector {
    P4OFProgram* model;
    // Result is deposited here.
    const IR::IOF_Node* currentTranslation;
    // The same expression is sometimes translated differently if
    // doing a match or generating an action.
    bool translateMatch;

 public:
    explicit ActionTranslator(P4OFProgram* model): model(model) {}

    bool preorder(const IR::PathExpression* path) override {
        auto decl = model->refMap->getDeclaration(path->path, true);
        auto reg = model->resources.getRegister(decl);
        if (reg) {
            currentTranslation = reg;
        } else if (decl->is<IR::Parameter>()) {
            // action parameters are translated to DDlog variables with the same name
            currentTranslation = new IR::OF_Fieldname(decl->getName());
        } else {
            ::error(ErrorType::ERR_INVALID, "%1%: could not translate expression", path);
        }
        if (translateMatch) {
            // TODO: booleans should be lowered into bit<1> values by the midend
            auto type = model->typeMap->getType(path, true);
            if (type->is<IR::Type_Boolean>()) {
                currentTranslation = new IR::OF_EqualsMatch(
                    currentTranslation->to<IR::OF_Expression>(),
                    new IR::OF_Constant(1));
            }
        }
        return false;
    }

    bool preorder(const IR::MethodCallExpression* mce) override {
        auto mi = P4::MethodInstance::resolve(mce, model->refMap, model->typeMap);
        if (auto bi = mi->to<P4::BuiltInMethod>()) {
            // we expect this to be a built-in method call on one of the headers.
            if (auto mem = mce->method->to<IR::Member>()) {
                if (auto parent = mem->expr->to<IR::Member>()) {
                    // All headers are two-level nested.
                    auto path = parent->expr->to<IR::PathExpression>();
                    auto baseDecl = model->refMap->getDeclaration(path->path, true);
                    if (baseDecl == model->ingress_hdr ||
                        baseDecl == model->egress_hdr) {
                        if (bi->name == "isValid" && translateMatch) {
                            currentTranslation = new IR::OF_ProtocolMatch(parent->member);
                            return false;
                        }
                    }
                }
            }
        }
        ::error(ErrorType::ERR_UNSUPPORTED_ON_TARGET,
                "%1%: expression not supported on target", mce);
        return false;
    }

    bool preorder(const IR::Member* member) override {
        currentTranslation = nullptr;
        if (auto path = member->expr->to<IR::PathExpression>()) {
            auto baseDecl = model->refMap->getDeclaration(path->path, true);
            auto baseType = model->typeMap->getType(baseDecl->getNode(), true);
            cstring name = member->member.name;
            if (baseDecl == model->ingress_meta_out ||
                baseDecl == model->ingress_meta ||
                baseDecl == model->egress_meta ||
                baseDecl == model->egress_meta_out ||
                baseDecl == model->ingress_itoa) {
                auto st = baseType->checkedTo<IR::Type_Struct>();
                auto field = st->getField(member->member);
                auto reg = model->resources.getRegister(field);
                CHECK_NULL(reg);
                currentTranslation = reg;
            } else if (baseDecl == model->ingress_meta_in ||
                       baseDecl == model->egress_meta_in) {
                if (name == "in_port")
                    currentTranslation = new IR::OF_Fieldname("in_port");
            }
        } else if (auto parent = member->expr->to<IR::Member>()) {
            // All headers are two-level nested.
            auto path = parent->expr->to<IR::PathExpression>();
            auto baseDecl = model->refMap->getDeclaration(path->path, true);
            if (baseDecl == model->ingress_hdr ||
                baseDecl == model->egress_hdr) {
                if (translateMatch)
                    currentTranslation = new IR::OF_Fieldname(
                        parent->member + "," + member->member);
                else
                    currentTranslation = new IR::OF_Fieldname(member->member);
            }
        }
        if (!currentTranslation) {
            ::error(ErrorType::ERR_UNKNOWN, "%1%: unknown implementation", member);
            return false;
        }
        if (translateMatch) {
            auto type = model->typeMap->getType(member, true);
            if (type->is<IR::Type_Boolean>()) {
                currentTranslation = new IR::OF_EqualsMatch(
                    currentTranslation->to<IR::OF_Expression>(),
                    new IR::OF_Constant(1));
            }
        }
        return false;
    }

    bool preorder(const IR::Equ* expression) override {
        auto left = _translate(expression->left);
        auto right = _translate(expression->right);
        if (!left || !right)
            return false;
        currentTranslation = new IR::OF_EqualsMatch(
            left->checkedTo<IR::OF_Expression>(),
            right->checkedTo<IR::OF_Expression>());
        return false;
    }

    bool preorder(const IR::LAnd* expression) override {
        auto left = _translate(expression->left);
        auto right = _translate(expression->right);
        if (!left || !right)
            return false;
        currentTranslation = new IR::OF_SeqMatch(
            left->checkedTo<IR::OF_Match>(),
            right->checkedTo<IR::OF_Match>());
        return false;
    }

    bool preorder(const IR::Slice* expression) override {
        auto e0 = _translate(expression->e0);
        auto hi = expression->getH();
        auto lo = expression->getL();
        if (!e0)
            return false;
        currentTranslation = new IR::OF_Slice(
            e0->checkedTo<IR::OF_Expression>(), hi, lo);
        return false;
    }

    bool preorder(const IR::Expression* expression) override {
        ::error(ErrorType::ERR_UNSUPPORTED_ON_TARGET,
                "%1%: expression not supported on target", expression);
        return false;
    }

    bool preorder(const IR::Constant* expression) override {
        currentTranslation = new IR::OF_Constant(expression);
        return false;
    }

    bool preorder(const IR::BoolLiteral* expression) override {
        currentTranslation = new IR::OF_Constant(expression->value ? 1 : 0);
        return false;
    }

    bool preorder(const IR::Cast* expression) override {
        // TODO: casts should be lowered into slices if possible
        currentTranslation = _translate(expression->expr);
        return false;
    }

    bool preorder(const IR::AssignmentStatement* statement) override {
        auto left = _translate(statement->left);
        auto right = _translate(statement->right);
        if (left && right) {
            auto lefte = left->to<IR::OF_Expression>();
            auto righte = right->to<IR::OF_Expression>();
            if (lefte && righte) {
                if (statement->right->is<IR::Literal>())
                    currentTranslation = new IR::OF_LoadAction(righte, lefte);
                else
                    currentTranslation = new IR::OF_MoveAction(righte, lefte);
            }
        }
        return false;
    }

    bool preorder(const IR::MethodCallStatement* mcs) override {
        auto mce = mcs->methodCall;
        auto mi = P4::MethodInstance::resolve(mce, model->refMap, model->typeMap);
        if (auto bi = mi->to<P4::BuiltInMethod>()) {
            // we expect this to be a built-in method call on one of the headers.
            if (auto mem = mce->method->to<IR::Member>()) {
                if (auto parent = mem->expr->to<IR::Member>()) {
                    // All headers are two-level nested.
                    auto path = parent->expr->to<IR::PathExpression>();
                    auto baseDecl = model->refMap->getDeclaration(path->path, true);
                    if (baseDecl == model->ingress_hdr ||
                        baseDecl == model->egress_hdr) {
                        if (bi->name == "setInvalid") {
                            if (mem->member == "vlan") {
                                currentTranslation = new IR::OF_ExplicitAction("strip_vlan");
                                return true;
                            }
                        } else if (bi->name == "setValid") {
                            // TODO: handle all known header insertions known
                        }
                    }
                }
            }
        }
        ::error(ErrorType::ERR_UNSUPPORTED_ON_TARGET,
                "%1%: expression not supported on target", mce);
        return false;
    }

    bool preorder(const IR::EmptyStatement*) override {
        currentTranslation = new IR::OF_EmptyAction();
        return false;
    }

    bool preorder(const IR::BlockStatement* block) override {
        const IR::OF_Action* translation = new IR::OF_EmptyAction();
        for (auto s : block->components) {
            auto act = _translate(s);
            if (act != nullptr) {
                auto acta = act->checkedTo<IR::OF_Action>();
                translation = new IR::OF_SeqAction(translation, acta);
            }
        }
        currentTranslation = translation;
        return false;
    }

    bool preorder(const IR::Statement* statement) override {
        ::error(ErrorType::ERR_UNSUPPORTED_ON_TARGET,
                "%1%: statement not supported on target", statement);
        return false;
    }

    bool preorder(const IR::ExitStatement*) override {
        currentTranslation = new IR::OF_ResubmitAction(model->egressExitId);
        return false;
    }

    const IR::IOF_Node* _translate(const IR::Node* node) {
        currentTranslation = nullptr;
        visit(node);
        return currentTranslation;
    }

    const IR::IOF_Node* translate(const IR::Node* node, bool match) {
        currentTranslation = nullptr;
        translateMatch = match;
        node->apply(*this);
        return currentTranslation;
    }
};

static const IR::DDlogAtom* makeFlowAtom(const IR::OF_MatchAndAction* value) {
    auto opt = value->apply(OpenFlowSimplify());
    auto str = new IR::DDlogStringLiteral(opt->toString());
    auto atom = new IR::DDlogAtom("Flow", new IR::DDlogTupleExpression({str}));
    return atom;
}

// Make a rule that contain a single atom.
static IR::DDlogRule* makeFlowRule(const IR::OF_MatchAndAction* flowRule, cstring comment) {
    auto atom = makeFlowAtom(flowRule);
    auto rule = new IR::DDlogRule(atom, {}, comment);
    return rule;
}

static cstring makeId(cstring name) {
    return name.replace(".", "_");
}

static cstring genTableName(const IR::P4Table* table) {
    return makeId(table->externalName()).capitalize();
}

// A table has a priority field in the control-plane if any of the keys
// has a match_kind which is not "exact".
static bool tableHasPriority(const IR::P4Table* table) {
    auto key = table->getKey();
    if (!key)
        return false;
    for (auto ke : key->keyElements) {
        auto match = ke->matchType;
        if (match->path->name.name != "exact")
            return true;
    }
    return false;
}

/// Generates code for DDlog declarations.
class DeclarationGenerator : public Inspector {
    P4OFProgram* model;
    IR::Vector<IR::Node> *declarations;
    IR::Vector<IR::Type> *alternatives;
    cstring tableName;
    ActionTranslator *actionTranslator;

 public:
    DeclarationGenerator(P4OFProgram* model, IR::Vector<IR::Node> *declarations):
            model(model), declarations(declarations) {
        setName("DeclarationGenerator");
        actionTranslator = new ActionTranslator(model);
    }

    Visitor::profile_t init_apply(const IR::Node* node) override {
        // Declare 'Flow' relation
        auto params = new IR::IndexedVector<IR::Parameter>();
        params->push_back(new IR::Parameter(
            IR::ID("flow"), IR::Direction::None, IR::Type_String::get()));
        declarations->push_back(new IR::DDlogRelation(
            IR::ID("Flow"), IR::Direction::Out, *params));

        // Declare 'MulticastGroup' relation
        params = new IR::IndexedVector<IR::Parameter>();
        params->push_back(new IR::Parameter(
            IR::ID("mcast_id"), IR::Direction::None, IR::Type_Bits::get(16)));
        params->push_back(new IR::Parameter(
            IR::ID("port"), IR::Direction::None, IR::Type_Bits::get(9)));
        declarations->push_back(new IR::DDlogRelation(
            IR::ID("MulticastGroup"), IR::Direction::In, *params));

        // initialize metadata values
        auto flowRule = new IR::OF_MatchAndAction(
            new IR::OF_TableMatch(0),
            new IR::OF_SeqAction(
                new IR::OF_SeqAction(
                    new IR::OF_LoadAction(
                        new IR::OF_Constant(0), model->outputPortRegister),
                    new IR::OF_LoadAction(
                        new IR::OF_Constant(0), model->multicastRegister)),
                new IR::OF_ResubmitAction(model->startIngressId)));
        declarations->push_back(makeFlowRule(flowRule, "initialize output port and output group"));
        return Inspector::init_apply(node);
    }

    bool preorder(const IR::Type_Typedef* tdef) override {
        auto trans = new IR::DDlogTypedef(tdef->name, tdef->type);
        declarations->push_back(trans);
        return true;
    }

    bool preorder(const IR::P4Table* table) override {
        tableName = genTableName(table);
        alternatives = nullptr;
        return true;
    }

    bool preorder(const IR::ActionListElement* ale) override {
        if (!alternatives)
            alternatives = new IR::Vector<IR::Type>();
        auto mce = ale->expression->to<IR::MethodCallExpression>();
        BUG_CHECK(mce, "%1%: expected a method call", ale->expression);
        auto mi = P4::MethodInstance::resolve(mce, model->refMap, model->typeMap);
        auto ac = mi->to<P4::ActionCall>();

        /// Generate a type in union type for the table declaration
        cstring alternative = makeId(tableName + "Action" + ac->action->externalName());
        auto fields = new IR::IndexedVector<IR::StructField>();
        BUG_CHECK(mce->arguments->size() == 0, "%1%: expected no arguments", mce);
        for (auto p : ac->action->parameters->parameters) {
            auto field = new IR::StructField(p->srcInfo, p->name, p->type);
            fields->push_back(field);
        }
        auto st = new IR::DDlogTypeStruct(ale->srcInfo, IR::ID(alternative), *fields);
        alternatives->push_back(st);
        return false;
    }

    void postorder(const IR::P4Table* table) override {
        cstring typeName = tableName + "Action";

        auto key = table->getKey();
        auto entries = table->getEntries();
        bool hasPriority = tableHasPriority(table);
        if (key && !entries) {
            // Union type representing all possible actions
            auto type = new IR::DDlogTypeAlt(*alternatives);
            auto td = new IR::DDlogTypedef(table->srcInfo, typeName, type);
            declarations->push_back(td);

            // Parameters of the corresponding P4Runtime relation
            auto params = new IR::IndexedVector<IR::Parameter>();
            // Arguments of a tuple expression
            for (auto ke : key->keyElements) {
                auto type = model->typeMap->getType(ke->expression, true);
                /*
                auto match = ke->matchType;
                  TODO: handle the various match_kinds
                if (match->path->name.name == "optional") {
                    type = new IR::DDlogTypeOption(type);
                }
                */
                auto name = ke->annotations->getSingle(
                    IR::Annotation::nameAnnotation)->getSingleString();
                auto param = new IR::Parameter(ke->srcInfo, name, IR::Direction::None, type);
                params->push_back(param);
            }
            if (hasPriority)
                params->push_back(new IR::Parameter(
                    "priority", IR::Direction::None, IR::Type_Bits::get(32)));
            params->push_back(new IR::Parameter(
                "action", IR::Direction::None, new IR::Type_Name(typeName)));
            auto rel = new IR::DDlogRelation(
                table->srcInfo, IR::ID(tableName), IR::Direction::In, *params);
            declarations->push_back(rel);
        }
        tableName = "";
    }
};

static CFG::Node* findActionSuccessor(const CFG::Node* node, const IR::P4Action* action) {
    for (auto e : node->successors.edges) {
        if (e->isUnconditional()) {
            return e->endpoint;
        } else if (e->isBool()) {
            return nullptr;
        } else {
            // switch statement
            if (e->label == action->name) {
                return e->endpoint;
            }
        }
    }
    return nullptr;
}

/// Generates DDlog Flow rules
class FlowGenerator : public Inspector {
    P4OFProgram* model;
    IR::Vector<IR::Node>* declarations;
    ActionTranslator* actionTranslator;

 public:
    FlowGenerator(P4OFProgram* model, IR::Vector<IR::Node> *declarations):
            model(model), declarations(declarations) {
        setName("FlowGenerator");
        CHECK_NULL(model); CHECK_NULL(declarations);
        actionTranslator = new ActionTranslator(model);
    }

    void convertTable(CFG::TableNode* table) {
        LOG2("Converting " << table);
        size_t id = table->id;
        auto p4table = table->table;
        auto keys = p4table->getKey();
        auto entries = p4table->getEntries();
        auto actions = p4table->getActionList();
        cstring tableName = genTableName(p4table);
        bool hasPriority = tableHasPriority(p4table);

        auto actionCases = new IR::Vector<IR::DDlogMatchCase>();
        auto args = new IR::Vector<IR::DDlogExpression>();
        if (keys) {
            for (auto ke : keys->keyElements) {
                auto name = ke->annotations->getSingle(
                    IR::Annotation::nameAnnotation)->getSingleString();
                auto varName = new IR::DDlogVarName(name);
                args->push_back(varName);
            }
        }
        if (hasPriority)
            args->push_back(new IR::DDlogVarName("priority"));
        args->push_back(new IR::DDlogVarName("action"));

        for (auto ale : actions->actionList) {
            auto mce = ale->expression->to<IR::MethodCallExpression>();
            BUG_CHECK(mce, "%1%: expected a method call", ale->expression);
            auto mi = P4::MethodInstance::resolve(mce, model->refMap, model->typeMap);
            auto ac = mi->to<P4::ActionCall>();

            CFG::Node* next = findActionSuccessor(table, ac->action);
            // BUG_CHECK(next, "%1%: no successor", p4table);
            // TODO
            auto successor = new IR::OF_ResubmitAction(next ? next->id : 0);

            /// Generate matching code for the rule
            std::vector<cstring> args;
            for (auto p : ac->action->parameters->parameters) {
                args.push_back(p->name);
            }
            cstring alternative = makeId(tableName + "Action" + ac->action->externalName());
            auto cExp = new IR::DDlogConstructorExpression(alternative, args);
            auto body = actionTranslator->translate(ac->action->body, false);
            auto action = body->checkedTo<IR::OF_Action>();
            action = new IR::OF_SeqAction(action, successor);
            auto opt = action->apply(OpenFlowSimplify());
            auto matched = new IR::DDlogStringLiteral(opt->toString());
            auto mc = new IR::DDlogMatchCase(cExp, matched);
            actionCases->push_back(mc);
        }

        if (!entries) {
            /// Table has no const entries: generate OF rules dynamically
            const IR::OF_Match* match = new IR::OF_TableMatch(id);
            // key evaluation
            if (keys) {
                for (auto k : keys->keyElements) {
                    auto key = actionTranslator->translate(k->expression, false);
                    if (key == nullptr)
                        return;
                    // The parameter name generated above for the corresponding key field
                    auto name = k->annotations->getSingle(
                        IR::Annotation::nameAnnotation)->getSingleString();
                    auto varName = new IR::OF_InterpolatedVarExpression(name);
                    match = new IR::OF_SeqMatch(
                        match,
                        new IR::OF_EqualsMatch(
                            key->checkedTo<IR::OF_Expression>(),
                            varName));
                }
            }

            if (hasPriority) {
                auto prio = new IR::OF_EqualsMatch(
                    new IR::OF_Fieldname("priority"),
                    new IR::OF_InterpolatedVarExpression("priority"));
                match = new IR::OF_SeqMatch(match, prio);
            }
            auto flowRule = new IR::OF_MatchAndAction(
                match,
                new IR::OF_InterpolatedVariableAction("actions"));
            auto flowTerm = makeFlowAtom(flowRule);
            auto ruleRhs = new IR::Vector<IR::DDlogTerm>();
            auto relationTerm = new IR::DDlogAtom(p4table->srcInfo, IR::ID(tableName),
                                                  new IR::DDlogTupleExpression(*args));
            // TODO: how do tables represent default_action values?
            if (keys)
                ruleRhs->push_back(relationTerm);

            const IR::DDlogExpression* computeAction;
            if (actionCases->size() == 0) {
                BUG("%1%: table with empty actions list", p4table);
            } else if (actionCases->size() == 1) {
                // no DDlog "match" needed
                computeAction = actionCases->at(0)->result;
            } else {
                computeAction = new IR::DDlogMatchExpression(
                    new IR::DDlogVarName("action"), *actionCases);
            }
            auto set = new IR::DDlogSetExpression("actions", computeAction);
            ruleRhs->push_back(new IR::DDlogExpressionTerm(set));
            auto rule = new IR::DDlogRule(flowTerm, *ruleRhs, p4table->externalName());
            declarations->push_back(rule);
        } else {
            // Table has constant entries: generate a fixed rule
            for (auto entry : entries->entries) {
                const IR::OF_Match* match = new IR::OF_TableMatch(id);
                BUG_CHECK(keys->keyElements.size() == entry->getKeys()->size(),
                          "%1%: mismatched keys and entry %2%", keys, entry);
                auto it = entry->getKeys()->components.begin();
                for (auto k : keys->keyElements) {
                    auto v = *it++;
                    auto key = actionTranslator->translate(k->expression, true);
                    auto value = actionTranslator->translate(v, true);
                    match = new IR::OF_SeqMatch(
                        match,
                        new IR::OF_EqualsMatch(
                            key->checkedTo<IR::OF_Expression>(),
                            value->checkedTo<IR::OF_Expression>()));
                }
                auto actionCall = entry->getAction()->checkedTo<IR::MethodCallExpression>();
                auto mi = P4::MethodInstance::resolve(actionCall, model->refMap, model->typeMap);
                auto ac = mi->to<P4::ActionCall>();
                CHECK_NULL(ac);
                auto spec = ac->specialize(model->refMap);
                CHECK_NULL(spec);
                auto callTranslation = actionTranslator->translate(spec->body, false);
                auto ofaction = callTranslation->checkedTo<IR::OF_Action>();

                CFG::Node* next = findActionSuccessor(table, ac->action);
                // BUG_CHECK(next, "%1%: no successor", p4table);
                // TODO
                auto successor = new IR::OF_ResubmitAction(next ? next->id : 0);
                ofaction = new IR::OF_SeqAction(ofaction, successor);
                auto flowRule = new IR::OF_MatchAndAction(match, ofaction);
                declarations->push_back(makeFlowRule(flowRule, p4table->name));
            }
        }
    }

    void convertDummy(CFG::DummyNode* node) {
        for (auto e : node->successors.edges) {
            // We really expect only one or no successor
            auto ma = new IR::OF_MatchAndAction(
                new IR::OF_TableMatch(node->id),
                new IR::OF_ResubmitAction(e->endpoint->id));
            auto rule = makeFlowRule(ma, nullptr);
            declarations->push_back(rule);
        }
    }

    void convertIf(CFG::IfNode* node) {
        LOG2("Converting " << node);
        size_t id = node->id;
        auto expr = actionTranslator->translate(node->statement->condition, true);

        for (auto e : node->successors.edges) {
            const IR::OF_Match* match = new IR::OF_TableMatch(id);
            CFG::Node* next = e->endpoint;
            auto action = new IR::OF_ResubmitAction(next->id);
            const IR::OF_MatchAndAction* ma;
            if (e->getBool()) {
                // if condition is true
                if (expr != nullptr) {
                    auto cond = expr->to<IR::OF_Match>();
                    match = new IR::OF_SeqMatch(
                        new IR::OF_SeqMatch(match, cond),
                        new IR::OF_EqualsMatch(
                            new IR::OF_Fieldname("priority"), new IR::OF_Constant(100)));
                }
                ma = new IR::OF_MatchAndAction(match, action);
            } else {
                // if condition is false
                ma = new IR::OF_MatchAndAction(
                    new IR::OF_SeqMatch(
                        match, new IR::OF_EqualsMatch(
                            new IR::OF_Fieldname("priority"), new IR::OF_Constant(1))),
                    action);
            }
            auto rule = makeFlowRule(ma, node->statement->toString());
            declarations->push_back(rule);
        }
    }

    void generate(CFG &cfg) {
        for (auto node : cfg.allNodes) {
            if (auto tn = node->to<CFG::TableNode>())
                convertTable(tn);
            else if (auto in = node->to<CFG::IfNode>())
                convertIf(in);
            else if (auto d = node->to<CFG::DummyNode>())
                convertDummy(d);
            else
                BUG("Unexpected CFG node %1%", node);
        }
    }
};

const IR::DDlogFunction* regDeclAsDDlog(const IR::OF_Register* reg,
                                        IR::Vector<IR::Node>* ddlog) {
    if (!reg || reg->friendlyName.isNullOrEmpty())
        return nullptr;
    auto result = new IR::DDlogFunction(
        IR::ID("r_" + reg->friendlyName),
        new IR::DDlogTypeString(),
        new IR::ParameterList(),
        new IR::DDlogStringLiteral(reg->canonicalName()));
    ddlog->push_back(result);
    return result;
}

class ResourceAllocator : public Inspector {
    OFResources& resources;
    IR::Vector<IR::Node>* ddlog;
 public:
    ResourceAllocator(OFResources& resources, IR::Vector<IR::Node>* ddlog):
            resources(resources), ddlog(ddlog) {}
    bool preorder(const IR::Declaration_Variable* decl) {
        auto reg = resources.allocateRegister(decl);
        regDeclAsDDlog(reg, ddlog);
        return false;
    }
};

P4OFProgram::P4OFProgram(const IR::P4Program* program, const IR::ToplevelBlock* top,
                P4::ReferenceMap* refMap, P4::TypeMap* typeMap):
        program(program), top(top), refMap(refMap), typeMap(typeMap), resources(typeMap) {
    CHECK_NULL(refMap); CHECK_NULL(typeMap); CHECK_NULL(top); CHECK_NULL(program);
}

void P4OFProgram::addFixedRules(IR::Vector<IR::Node> *declarations) {
    // drop if output port is 0
    auto flowRule = new IR::OF_MatchAndAction(
        new IR::OF_SeqMatch(
            new IR::OF_TableMatch(egressExitId),
            new IR::OF_SeqMatch(
                new IR::OF_EqualsMatch(
                    outputPortRegister,
                    new IR::OF_Constant(0)),
                new IR::OF_EqualsMatch(
                    new IR::OF_Fieldname("priority"),
                    new IR::OF_Constant(100)))),
        new IR::OF_DropAction());
    declarations->push_back(makeFlowRule(flowRule, "drop if output port is 0"));

    // send to output port from dedicated register
    flowRule = new IR::OF_MatchAndAction(
        new IR::OF_TableMatch(egressExitId),
        new IR::OF_OutputAction(outputPortRegister));
    declarations->push_back(makeFlowRule(flowRule, "send to chosen port"));

    // jump to multicast table
    flowRule = new IR::OF_MatchAndAction(
        new IR::OF_TableMatch(ingressExitId),
        new IR::OF_ResubmitAction(multicastId));
    declarations->push_back(makeFlowRule(flowRule, "jump to multicast table"));

    // Fixed implementation of multicast table:
    // - multicast group is 0 - just forward to egress
    flowRule = new IR::OF_MatchAndAction(
        new IR::OF_SeqMatch(
            new IR::OF_TableMatch(multicastId),
            new IR::OF_EqualsMatch(multicastRegister, new IR::OF_Constant(0))),
        new IR::OF_ResubmitAction(egressStartId));
    declarations->push_back(makeFlowRule(flowRule, "if multicast group is 0 just forward"));
    // - multicast group non-zero: clone packet for each row from the MuticastGroup table
    flowRule = new IR::OF_MatchAndAction(
        new IR::OF_SeqMatch(
            new IR::OF_TableMatch(multicastId),
            new IR::OF_EqualsMatch(multicastRegister,
                                   new IR::OF_InterpolatedVarExpression("mcast_id"))),
        new IR::OF_InterpolatedVariableAction("outputs"));
    auto lhs = makeFlowAtom(flowRule);

    auto lookupGroup = new IR::DDlogAtom(
        "MulticastGroup", new IR::DDlogTupleExpression(
            {new IR::DDlogVarName("mcast_id"), new IR::DDlogVarName("port")}));

    auto clone = new IR::OF_CloneAction(
        new IR::OF_SeqAction(
            new IR::OF_MoveAction(
                new IR::OF_InterpolatedVarExpression("port"),
                outputPortRegister),
            new IR::OF_ResubmitAction(multicastId)));
    // TODO: This is not an accurate representation of the DDlog IR tree,
    // but it generates the same textual representation.
    auto outputs = new IR::DDlogSetExpression(
        "outputs",
        new IR::DDlogApply(
            "join",
            new IR::DDlogApply(
                "to_vec",
                new IR::DDlogApply(
                    "group_by",
                    new IR::DDlogStringLiteral(clone->toString()),
                    { new IR::DDlogVarName("mcast_id") }),
                {}),
            { new IR::DDlogStringLiteral(", ") }));
    auto rule = new IR::DDlogRule(lhs, { lookupGroup, new IR::DDlogExpressionTerm(outputs) },
                                  "multicast");
    declarations->push_back(rule);
}

static const IR::Type_Struct* getStructType(P4::TypeMap* typeMap, const IR::Parameter* param) {
    auto t = typeMap->getType(param);
    if (t == nullptr)
        return nullptr;
    auto res = t->to<IR::Type_Struct>();
    if (!res)
        ::error(ErrorType::ERR_MODEL,
                "%1%: expected a struct type, not %2%", param, t);
    return res;
}

void P4OFProgram::build() {
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
    if (params->size() != 5) {
        ::error(ErrorType::ERR_EXPECTED,
                "Expected ingress block %1% to have exactly 5 parameters", ingress);
        return;
    }

    auto eg = pack->getParameterValue("eg")->checkedTo<IR::ControlBlock>();
    if (!eg)
        ::error(ErrorType::ERR_MODEL, "No parameter named 'eg' for OfSwitch package.");
    egress = eg->container;

    auto it = params->parameters.begin();
    ingress_hdr = *it; ++it;
    ingress_meta = *it; ++it;
    ingress_meta_in = *it; ++it;
    ingress_itoa = *it; ++it;
    ingress_meta_out = *it;

    Headers = getStructType(typeMap, ingress_hdr);  // a struct full of headers
    input_metadata_t = getStructType(typeMap, ingress_meta_in);
    M = getStructType(typeMap, ingress_meta);
    output_metadata_t = getStructType(typeMap, ingress_meta_out);
    ingress_to_arch_t = getStructType(typeMap, ingress_itoa);

    params = egress->type->applyParams;
    if (params->size() != 4) {
        ::error(ErrorType::ERR_EXPECTED,
                "Expected egress block %1% to have exactly 4 parameters", egress);
        return;
    }
    it = params->parameters.begin();
    egress_hdr = *it; ++it;
    egress_meta = *it; ++it;
    egress_meta_in = *it; ++it;
    egress_meta_out = *it;
}

IR::DDlogProgram* P4OFProgram::convert() {
    // Collect here the DDlog program
    auto decls = new IR::Vector<IR::Node>();

    for (auto sf : output_metadata_t->fields) {
        auto reg = resources.allocateRegister(sf);
        regDeclAsDDlog(reg, decls);
        if (sf->name == "out_port")
            outputPortRegister = reg;
    }
    for (auto sf : ingress_to_arch_t->fields) {
        auto reg = resources.allocateRegister(sf);
        regDeclAsDDlog(reg, decls);
        if (sf->name == "out_group")
            multicastRegister = reg;
    }
    for (auto sf : M->fields) {
        auto reg = resources.allocateRegister(sf);
        (void)regDeclAsDDlog(reg, decls);
    }

    CHECK_NULL(outputPortRegister);
    CHECK_NULL(multicastRegister);

    ResourceAllocator allocator(resources, decls);
    ingress->apply(allocator);
    egress->apply(allocator);

    ingress_cfg.build(ingress, refMap, typeMap);
    auto multicastNode = new CFG::DummyNode("multicast");
    egress_cfg.build(egress, refMap, typeMap);

    // Here we take advantage of the fact that node ids are not reused
    // when building a new control flow graph.
    startIngressId = ingress_cfg.entryPoint->id;
    ingressExitId = ingress_cfg.exitPoint->id;
    multicastId = multicastNode->id;
    egressStartId = egress_cfg.entryPoint->id;
    egressExitId = egress_cfg.exitPoint->id;

    DeclarationGenerator dgen(this, decls);
    program->apply(dgen);

    FlowGenerator rgen(this, decls);
    rgen.generate(ingress_cfg);
    rgen.generate(egress_cfg);
    addFixedRules(decls);

    auto result = new IR::DDlogProgram(decls);
    return result;
}

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
