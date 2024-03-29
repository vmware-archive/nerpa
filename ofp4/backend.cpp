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
#include "ofvisitors.h"
#include "ir/ir.h"
#include "lib/sourceCodeBuilder.h"
#include "lib/nullstream.h"
#include "frontends/p4/evaluator/evaluator.h"
#include "frontends/p4/methodInstance.h"
#include "frontends/p4/evaluator/substituteParameters.h"
#include "frontends/p4/parameterSubstitution.h"
#include "resources.h"

namespace OFP4 {

/// Can be used to translate action bodies or expressions into OF actions/expressions
class ActionTranslator : public Inspector {
    OFP4Program* model;
    // Result is deposited here.
    const IR::IOF_Node* currentTranslation;
    // The same expression is sometimes translated differently if
    // doing a match or generating an action.
    bool translateMatch;
    size_t exitBlockId = 0;
    const P4::ParameterSubstitution* substitution;

 public:
    ActionTranslator(OFP4Program* model,
                     const P4::ParameterSubstitution* substitution = nullptr):
            model(model), substitution(substitution) {
        visitDagOnce = false;
    }

    bool preorder(const IR::Parameter* param) override {
        currentTranslation = nullptr;
        if (substitution) {
            auto arg = substitution->lookup(param);
            if (!arg)
                return false;
            visit(arg->expression);
        }
        return false;
    }

    bool preorder(const IR::PathExpression* path) override {
        auto decl = model->refMap->getDeclaration(path->path, true);
        auto reg = model->resources.getRegister(decl);
        auto type = model->typeMap->getType(path, true);
        if (reg) {
            currentTranslation = reg;
        } else if (decl->is<IR::Parameter>()) {
            // action parameters are translated to DDlog variables with the same name
            currentTranslation = new IR::OF_InterpolatedVarExpression(
                decl->getName(), type->width_bits());
        } else {
            ::error(ErrorType::ERR_INVALID, "%1%: could not translate expression", path);
        }
        if (translateMatch) {
            // TODO: booleans should be lowered into bit<1> values by the midend
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
        const IR::Annotation *prereq = nullptr;
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
                    currentTranslation = new IR::OF_Register("in_port", 16, 0, 15, false);
            }
        } else if (auto parent = member->expr->to<IR::Member>()) {
            // All headers are two-level nested.
            auto path = parent->expr->to<IR::PathExpression>();
            auto baseDecl = model->refMap->getDeclaration(path->path, true);
            if (baseDecl == model->ingress_hdr ||
                baseDecl == model->egress_hdr) {
                auto parentType = model->typeMap->getType(member->expr, true);
                auto st = parentType->checkedTo<IR::Type_StructLike>();
                auto field = st->getField(member->member);
                BUG_CHECK(field, "%1% unexpectedly lacks member %2%", st, field);

                // This field might be a slice of an OpenFlow field or it might
                // be the whole field.
                int size, low, high;
                if (auto slice = field->getAnnotation("of_slice")) {
                    if (slice->expr.size() != 3) {
                        ::error(ErrorType::ERR_EXPECTED, "%1%: @of_slice must contain 3 constants", slice);
                        return false;
                    }
                    int i = 0;
                    for (auto x : { &low, &high, &size }) {
                        auto elem = slice->expr[i++];
                        auto value = elem->to<IR::Constant>();
                        if (elem == nullptr) {
                            ::error(ErrorType::ERR_EXPECTED,
                                    "%1%: %2% is not a constant in @of_slice", slice, elem);
                            return false;
                        }
                        *x = value->asInt();
                    }
                    if (!(0 <= low && low <= high && high < size)) {
                        ::error(ErrorType::ERR_EXPECTED,
                                "%1%: @of_slice(low,high,size) requires 0 <= low <= high < size", slice);
                        return false;
                    }

                    int width = field->type->width_bits();
                    if (high - low + 1 != width) {
                        ::error(ErrorType::ERR_EXPECTED,
                                "%1%: @of_slice(low,high,size) is a %2%-bit slice "
                                "but %3% is a %4%-bit field.",
                                slice, high - low + 1, field, width);
                        return false;
                    }
                } else {
                    size = field->type->width_bits();
                    low = 0;
                    high = size - 1;
                }
                currentTranslation = new IR::OF_Register(field->externalName(),
                                                         size, low, high,
                                                         member->type->is<IR::Type_Boolean>());

                if (translateMatch) {
                    prereq = field->getAnnotation("of_prereq");
                    if (prereq == nullptr)
                        prereq = st->getAnnotation("of_prereq");
                }
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
        if (prereq != nullptr) {
            auto basic_match = currentTranslation;
            auto prereq_match = new IR::OF_PrereqMatch(prereq->getSingleString());
            auto sequence = new IR::OF_SeqMatch();
            sequence->push_back(basic_match->to<IR::OF_Match>());
            sequence->push_back(prereq_match->to<IR::OF_Match>());
            currentTranslation = sequence;
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
        auto sequence = new IR::OF_SeqMatch();
        sequence->push_back(left->checkedTo<IR::OF_Match>());
        sequence->push_back(right->checkedTo<IR::OF_Match>());
        currentTranslation = sequence;
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
        // Lower a narrowing cast into a slice.
        if (size_t width = expression->destType->width_bits()) {
            if (auto reg = expression->expr->to<IR::OF_Register>()) {
                if (width < reg->width()) {
                    currentTranslation = reg->lowBits(reg->width());
                    return false;
                }
            }
        }

        currentTranslation = _translate(expression->expr);
        return false;
    }

    bool preorder(const IR::AssignmentStatement* statement) override {
        auto dst = _translate(statement->left);
        auto src = _translate(statement->right);
        if (src && dst) {
            auto srce = src->to<IR::OF_Expression>();
            auto dste = dst->to<IR::OF_Expression>();
            if (srce && dste) {
                if (srce->is<IR::OF_Constant>() ||
                    srce->is<IR::OF_InterpolatedVarExpression>())
                    currentTranslation = new IR::OF_LoadAction(srce, dste);
                else {
                    int srcw = srce->width();
                    int dstw = dste->width();
                    if (srcw && dstw && srcw < dstw) {
                        auto dstr = dste->to<IR::OF_Register>();
                        if (dstr) {
                            /* To assign a short source to a wider destination,
                             * copy the low-order bits then zero the rest. */
                            currentTranslation = new IR::OF_SeqAction(
                                new IR::OF_MoveAction(srce, dstr->lowBits(srcw)),
                                new IR::OF_LoadAction(new IR::OF_Constant(0),
                                                      dstr->highBits(dstw - srcw)));
                            return false;
                        }
                    }
                    currentTranslation = new IR::OF_MoveAction(srce, dste);
                }
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
        currentTranslation = new IR::OF_ResubmitAction(exitBlockId);
        return false;
    }

    const IR::IOF_Node* _translate(const IR::Node* node) {
        currentTranslation = nullptr;
        visit(node);
        return currentTranslation;
    }

    const IR::IOF_Node* translate(const IR::Node* node, bool match, size_t exitId) {
        exitBlockId = exitId;
        currentTranslation = nullptr;
        translateMatch = match;
        node->apply(*this);
        return currentTranslation;
    }
};

static const IR::DDlogAtom* makeFlowAtom(const IR::OF_MatchAndAction* value) {
    auto opt = value->apply(OpenFlowSimplify());
    auto str = new IR::DDlogStringLiteral(OpenFlowPrint::toString(opt));
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
    OFP4Program* model;
    IR::Vector<IR::Node> *declarations;
    IR::Vector<IR::Type> *tableActions;
    IR::Vector<IR::Type> *defaultActions;
    cstring tableName;
    ActionTranslator *actionTranslator;

 public:
    DeclarationGenerator(OFP4Program* model, IR::Vector<IR::Node> *declarations):
            model(model), declarations(declarations) {
        setName("DeclarationGenerator"); visitDagOnce = false;
        actionTranslator = new ActionTranslator(model);
    }

    Visitor::profile_t init_apply(const IR::Node* node) override {
        // Declare 'Flow' relation
        declarations->push_back(new IR::DDlogRelationDirect(
            IR::ID("Flow"), IR::Direction::Out, new IR::Type_Name("flow_t")));

        // Declare 'Flow' index
        auto params = new IR::IndexedVector<IR::Parameter>();
        auto param = new IR::Parameter("s", IR::Direction::None, new IR::DDlogTypeString());
        params->push_back(param);
        auto formals = new std::vector<IR::ID>();
        formals->push_back("s");
        declarations->push_back(new IR::DDlogIndex(IR::ID("Flow"), *params, "Flow", *formals));

        // Declare 'MulticastGroup' relation
        declarations->push_back(new IR::DDlogRelationDirect(
            IR::ID("MulticastGroup"), IR::Direction::In, new IR::Type_Name("multicast_group_t")));

        // TODO: maybe this table should be removed
        auto flowRule = new IR::OF_MatchAndAction(
            new IR::OF_TableMatch(0),
            new IR::OF_ResubmitAction(model->startIngressId));
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
        tableActions = new IR::Vector<IR::Type>();
        defaultActions = new IR::Vector<IR::Type>();
        return true;
    }

    bool preorder(const IR::ActionListElement* ale) override {
        auto annos = ale->getAnnotations();
        bool defaultOnly = annos->getSingle(
            IR::Annotation::defaultOnlyAnnotation) != nullptr;
        bool tableOnly = annos->getSingle(
            IR::Annotation::tableOnlyAnnotation) != nullptr;

        auto mce = ale->expression->to<IR::MethodCallExpression>();
        BUG_CHECK(mce, "%1%: expected a method call", ale->expression);
        auto mi = P4::MethodInstance::resolve(mce, model->refMap, model->typeMap);
        auto ac = mi->to<P4::ActionCall>();
        CHECK_NULL(ac);

        /// Generate a type in union type for the table declaration
        auto fields = new IR::IndexedVector<IR::StructField>();
        BUG_CHECK(mce->arguments->size() == 0, "%1%: expected no arguments", mce);
        for (auto p : ac->action->parameters->parameters) {
            auto field = new IR::StructField(p->srcInfo, p->name, p->type);
            fields->push_back(field);
        }
        if (!defaultOnly) {
            cstring alternative = makeId(tableName + "Action" + ac->action->name);
            auto st = new IR::DDlogTypeStruct(ale->srcInfo, IR::ID(alternative), *fields);
            tableActions->push_back(st);
        }
        if (!tableOnly) {
            cstring alternative = makeId(tableName + "DefaultAction" + ac->action->name);
            auto st = new IR::DDlogTypeStruct(ale->srcInfo, IR::ID(alternative), *fields);
            defaultActions->push_back(st);
        }
        return false;
    }

    void postorder(const IR::P4Table* table) override {
        cstring typeName = tableName + "Action";

        auto key = table->getKey();
        bool hasPriority = tableHasPriority(table);

        if (key) {
            // Union type representing all possible actions
            auto type = new IR::DDlogTypeAlt(*tableActions);
            auto td = new IR::DDlogTypedef(table->srcInfo, typeName, type);
            declarations->push_back(td);

            // Parameters of the corresponding P4Runtime relation
            auto params = new IR::IndexedVector<IR::Parameter>();
            // Arguments of a tuple expression
            for (auto ke : key->keyElements) {
                auto type = model->typeMap->getType(ke->expression, true);
                auto match = ke->matchType;
                if (match->path->name.name == "optional") {
                    type = new IR::DDlogTypeOption(type);
                }
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
            auto rel = new IR::DDlogRelationSugared(
                table->srcInfo, IR::ID(tableName), IR::Direction::In, *params);
            declarations->push_back(rel);
        }

        auto defaultAction = table->getDefaultAction();
        CHECK_NULL(defaultAction);  // always inserted by front-end
        CHECK_NULL(table->properties);
        auto daprop = table->properties->getProperty(
            IR::TableProperties::defaultActionPropertyName);
        CHECK_NULL(daprop);
        cstring daTypeName = typeName + "DefaultAction";
        auto type = new IR::DDlogTypeAlt(*defaultActions);
        auto td = new IR::DDlogTypedef(table->srcInfo, daTypeName, type);
        declarations->push_back(td);

        auto params = new IR::IndexedVector<IR::Parameter>();
        params->push_back(new IR::Parameter(
                              "action", IR::Direction::None, new IR::Type_Name(daTypeName)));
        auto rel = new IR::DDlogRelationSugared(
            table->srcInfo, IR::ID(tableName + "DefaultAction"), IR::Direction::In, *params);
        declarations->push_back(rel);

        tableName = "";
    }
};

static CFG::Node* findActionSuccessor(
    const CFG::Node* node, const IR::P4Action* action, bool defaultAction) {
    for (auto e : node->successors.edges) {
        if (e->isUnconditional()) {
            return e->endpoint;
        } else if (e->isBool()) {
            bool condition = e->getBool();
            if (condition && !defaultAction) {
                // missed
                return e->endpoint;
            } else if (!condition && defaultAction) {
                // hit
                return e->endpoint;
            }
        } else {
            // switch statement
            if (e->label == action->name) {
                return e->endpoint;
            }
        }
    }
    return nullptr;
}

static bool
defaultActionIsConstant(const IR::P4Table* p4table)
{
    auto daprop = p4table->properties->getProperty(
        IR::TableProperties::defaultActionPropertyName);
    CHECK_NULL(daprop);
    return daprop->isConstant;
}

/// Generates DDlog Flow rules
class FlowGenerator : public Inspector {
    OFP4Program* model;
    IR::Vector<IR::Node>* declarations;
    ActionTranslator* actionTranslator;
    size_t exitBlockId;

 public:
    FlowGenerator(OFP4Program* model, IR::Vector<IR::Node> *declarations):
            model(model), declarations(declarations) {
        setName("FlowGenerator"); visitDagOnce = false;
        CHECK_NULL(model); CHECK_NULL(declarations);
        actionTranslator = new ActionTranslator(model);
    }

    void generateActionCall(const IR::MethodCallExpression* actionCall,
                            const IR::OF_Match* match,
                            const CFG::TableNode* cfgtable,
                            bool defaultAction) {
        auto mi = P4::MethodInstance::resolve(actionCall, model->refMap, model->typeMap);
        auto ac = mi->to<P4::ActionCall>();
        CHECK_NULL(ac);
        auto at = new ActionTranslator(model, &ac->substitution);
        auto callTranslation = at->translate(ac->action->body, false, exitBlockId);
        auto ofaction = callTranslation->checkedTo<IR::OF_Action>();

        CFG::Node* next = findActionSuccessor(cfgtable, ac->action, defaultAction);
        BUG_CHECK(next, "%1%:%2%: no successor", cfgtable->table->name, ac->action->name);
        auto successor = new IR::OF_ResubmitAction(next ? next->id : 0);
        ofaction = new IR::OF_SeqAction(ofaction, successor);
        auto flowRule = new IR::OF_MatchAndAction(match, ofaction);
        declarations->push_back(makeFlowRule(flowRule, cfgtable->table->externalName()));
    }

    IR::Node*
    makeConstantEntry(const IR::ListExpression* keys, const IR::Expression* action,
                      const cstring& tableName, bool isDefault, cstring comment) {
        auto members = IR::Vector<IR::DDlogExpression>();

        if (keys) {
            for (auto v : keys->components) {
                auto value = actionTranslator->translate(v, true, exitBlockId);
                auto str = new IR::DDlogLiteral(OpenFlowPrint::toString(value->to<IR::Node>()));
                members.push_back(str->checkedTo<IR::DDlogExpression>());
            }
        }

        auto mce = action->checkedTo<IR::MethodCallExpression>();
        auto mi = P4::MethodInstance::resolve(mce, model->refMap, model->typeMap);
        auto ac = mi->to<P4::ActionCall>();

        auto method = makeId(tableName + (isDefault ? "Default" : "") + "Action" + ac->action->name);
        std::vector<cstring> args;
        for (auto arg : *mce->arguments) {
            auto ofArg = actionTranslator->translate(arg, true, 0);
            args.push_back(ofArg->toString());
        }
        auto cExp = new IR::DDlogConstructorExpression(method, args);
        members.push_back(cExp->checkedTo<IR::DDlogExpression>());

        auto atom = new IR::DDlogAtom(makeId(tableName + (isDefault ? "DefaultAction" : "")), new IR::DDlogTupleExpression(members));
        auto rule = new IR::DDlogRule(atom, {}, comment);
        return rule;
    }

    // This recursive function adds to 'this->declarations' a set of
    // "DDlogRule"s for the P4 'table'.  When called, 'tableArgs' contains set
    // of arguments that the caller has already figured out for the P4 'table'
    // on the right-hand side of the DDlog :-, and 'match' contains the set of
    // OpenFlow match expressions that the caller has already added
    // corresponding to the arguments.  'curKey'...'end' contains the key
    // elements still to be processed and recursively passed into this function.
    void convertKey(CFG::TableNode* table,
                    const IR::Vector<IR::DDlogMatchCase>* tableCases,
                    safe_vector<const IR::DDlogExpression*> tableArgs,
                    safe_vector<const IR::OF_Match*> match,
                    IR::Vector<IR::KeyElement>::const_iterator curKey,
                    IR::Vector<IR::KeyElement>::const_iterator end,
                    size_t nKeys) {
        if (curKey != end) {
            // Recursive case.
            auto k = *curKey;
            auto name = k->annotations->getSingle(
                IR::Annotation::nameAnnotation)->getSingleString();
            auto key = actionTranslator->translate(k->expression, false, exitBlockId);
            if (key == nullptr)
                return;

            auto matchType = k->matchType;
            if (matchType->path->name.name == "optional") {
                // For an optional field, we need a flow for None and a flow
                // for Some.  The flow for None doesn't have a match component;
                // add it first, recurse, and discard it.
                std::vector<cstring> args;
                tableArgs.push_back(new IR::DDlogConstructorExpression(IR::ID("None"), args));
                convertKey(table, tableCases, tableArgs, match, curKey + 1, end, nKeys);
                tableArgs.pop_back();

                // Then add the Some and fall through.  The match component
                // gets added just below in code shared with exact-match.
                args.push_back(name);
                tableArgs.push_back(new IR::DDlogConstructorExpression(IR::ID("Some"), args));
            } else {
                tableArgs.push_back(new IR::DDlogVarName(name));
            }

            auto keye = key->checkedTo<IR::OF_Expression>();
            auto varName = new IR::OF_InterpolatedVarExpression(name, keye->width());
            match.push_back(new IR::OF_EqualsMatch(keye, varName));

            convertKey(table, tableCases, tableArgs, match, curKey + 1, end, nKeys);

            return;
        }

        // Base case.
        auto p4table = table->table;
        if (tableHasPriority(p4table)) {
            tableArgs.push_back(new IR::DDlogVarName("priority"));
            match.push_back(
                new IR::OF_PriorityMatch(
                    new IR::OF_InterpolatedVarExpression("priority", 16)));
        }
        tableArgs.push_back(new IR::DDlogVarName("action"));

        auto seqMatch = new IR::OF_SeqMatch(IR::Vector<IR::OF_Match>(match));
        auto flowRule = new IR::OF_MatchAndAction(
            seqMatch,
            new IR::OF_InterpolatedVariableAction("actions"));
        auto flowTerm = makeFlowAtom(flowRule);
        auto ruleRhs = new IR::Vector<IR::DDlogTerm>();
        auto relationTerm = new IR::DDlogAtom(p4table->srcInfo,
                                              IR::ID(genTableName(p4table)),
                                              new IR::DDlogTupleExpression(*new IR::Vector<IR::DDlogExpression>(tableArgs)));
        if (nKeys)
            ruleRhs->push_back(relationTerm);

        const IR::DDlogExpression* computeAction;
        if (tableCases->size() == 0) {
            BUG("%1%: table with empty actions list", p4table);
        }
        else if (!nKeys && tableCases->size() == 1) {
            // no DDlog "match" needed
            computeAction = tableCases->at(0)->result;
        } else {
            computeAction = new IR::DDlogMatchExpression(
                new IR::DDlogVarName("action"), *tableCases);
        }
        auto set = new IR::DDlogSetExpression("actions", computeAction);
        ruleRhs->push_back(new IR::DDlogExpressionTerm(set));
        auto rule = new IR::DDlogRule(flowTerm, *ruleRhs, p4table->externalName());
        declarations->push_back(rule);
    }

    void convertTable(CFG::TableNode* table) {
        LOG2("Converting " << table);
        size_t id = table->id;
        auto p4table = table->table;
        auto entries = p4table->getEntries();
        auto actions = p4table->getActionList();
        cstring tableName = genTableName(p4table);
        const IR::OF_Match* tablematch = new IR::OF_TableMatch(id);

        auto tableCases = new IR::Vector<IR::DDlogMatchCase>();
        auto defaultCases = new IR::Vector<IR::DDlogMatchCase>();
        auto defaultArgs = new IR::Vector<IR::DDlogExpression>();

        auto acvar = new IR::DDlogVarName("action");
        defaultArgs->push_back(acvar);

        for (auto ale : actions->actionList) {
            auto mce = ale->expression->to<IR::MethodCallExpression>();
            BUG_CHECK(mce, "%1%: expected a method call", ale->expression);
            auto mi = P4::MethodInstance::resolve(mce, model->refMap, model->typeMap);
            auto ac = mi->to<P4::ActionCall>();
            auto annos = ale->getAnnotations();
            bool defaultOnly = annos->getSingle(
                IR::Annotation::defaultOnlyAnnotation) != nullptr;
            bool tableOnly = annos->getSingle(
                IR::Annotation::tableOnlyAnnotation) != nullptr;

            CFG::Node* next = findActionSuccessor(table, ac->action, false);
            BUG_CHECK(next, "%1%:%2%: no successor", p4table->name, ac->action->name);
            auto successor = new IR::OF_ResubmitAction(next ? next->id : 0);

            /// Generate matching code for the rule
            std::vector<cstring> keyargs;
            for (auto p : ac->action->parameters->parameters) {
                keyargs.push_back(p->name);
            }
            auto body = actionTranslator->translate(ac->action->body, false, exitBlockId);
            auto action = body->checkedTo<IR::OF_Action>();
            action = new IR::OF_SeqAction(action, successor);
            auto opt = action->apply(OpenFlowSimplify());
            auto matched = new IR::DDlogStringLiteral(OpenFlowPrint::toString(opt));

            if (!defaultOnly) {
                cstring alternative = makeId(tableName + "Action" + ac->action->name);
                auto cExp = new IR::DDlogConstructorExpression(alternative, keyargs);
                auto mc = new IR::DDlogMatchCase(cExp, matched);
                tableCases->push_back(mc);
            }
            if (!tableOnly) {
                cstring alternative = makeId(
                    tableName + "DefaultAction" + ac->action->name);
                auto cExp = new IR::DDlogConstructorExpression(alternative, keyargs);
                auto mc = new IR::DDlogMatchCase(cExp, matched);
                defaultCases->push_back(mc);
            }
        }

        auto key = table->table->getKey();
        if (!key) {
            key = new IR::Key(IR::Vector<IR::KeyElement>());
        }
        safe_vector<const IR::DDlogExpression*> tableArgs;
        safe_vector<const IR::OF_Match*> match;
        match.push_back(new IR::OF_TableMatch(table->id));
        convertKey(table, tableCases, tableArgs, match,
                   key->keyElements.begin(), key->keyElements.end(),
                   key->keyElements.size());

        // For each constant entry, add a constant value to the relation.
        if (entries) {
            for (auto entry : entries->entries) {
                declarations->push_back(makeConstantEntry(entry->getKeys(), entry->getAction(), tableName, false,
                                            "constant entry for table " + tableName));
            }
        }

        // Handle default action
        auto defaultAction = p4table->getDefaultAction();
        CHECK_NULL(defaultAction);  // always inserted by front-end
        auto default_match = new IR::OF_SeqMatch();
        default_match->push_back(tablematch);
        default_match->push_back(new IR::OF_PriorityMatch(new IR::OF_Constant(1)));

        auto flowRule = new IR::OF_MatchAndAction(
            default_match,
            new IR::OF_InterpolatedVariableAction("actions"));
        auto flowTerm = makeFlowAtom(flowRule);
        auto ruleRhs = new IR::Vector<IR::DDlogTerm>();
        auto relationTerm = new IR::DDlogAtom(
            p4table->srcInfo, IR::ID(tableName + "DefaultAction"),
            new IR::DDlogTupleExpression(*defaultArgs));
        ruleRhs->push_back(relationTerm);
        const IR::DDlogExpression* computeAction;
        if (defaultCases->size() == 0) {
            BUG("%1%: table with empty default actions list", p4table);
        } else if (defaultCases->size() == 1) {
            // no DDlog "match" needed
            computeAction = defaultCases->at(0)->result;
        } else {
            computeAction = new IR::DDlogMatchExpression(
                new IR::DDlogVarName("action"), *defaultCases);
        }
        auto set = new IR::DDlogSetExpression("actions", computeAction);
        ruleRhs->push_back(new IR::DDlogExpressionTerm(set));
        auto rule = new IR::DDlogRule(flowTerm, *ruleRhs, p4table->externalName());
        declarations->push_back(rule);

        if (defaultActionIsConstant(p4table)) {
            declarations->push_back(makeConstantEntry(nullptr, defaultAction, tableName, true,
                                        "constant default action for table " + tableName));
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
        auto expr = actionTranslator->translate(node->statement->condition, true, exitBlockId);

        for (auto e : node->successors.edges) {
            auto match = new IR::OF_SeqMatch();
            match->push_back(new IR::OF_TableMatch(id));
            CFG::Node* next = e->endpoint;
            auto action = new IR::OF_ResubmitAction(next ? next->id : 0);
            const IR::OF_MatchAndAction* ma;
            if (e->getBool()) {
                // if condition is true
                if (expr != nullptr) {
                    auto cond = expr->to<IR::OF_Match>();
                    match->push_back(cond);
                    match->push_back(
                        new IR::OF_PriorityMatch(new IR::OF_Constant(100)));
                }
                ma = new IR::OF_MatchAndAction(match, action);
            } else {
                // if condition is false
                match->push_back(
                    new IR::OF_PriorityMatch(new IR::OF_Constant(1)));
                ma = new IR::OF_MatchAndAction(match, action);
            }
            auto rule = makeFlowRule(ma, node->statement->toString());
            declarations->push_back(rule);
        }
    }

    void generate(CFG &cfg, size_t exitId) {
        exitBlockId = exitId;
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

/// Allocates a register and inserts a declaration for a function
/// returning the register in the DDlog program
const IR::OF_Register* allocateRegister(
    const IR::Declaration* decl,
    OFResources& resources,
    IR::Vector<IR::Node>* ddlog) {
    auto reg = resources.allocateRegister(decl);
    if (reg && !reg->friendlyName.isNullOrEmpty()) {
        auto ddfunc = new IR::DDlogFunction(
            IR::ID("r_" + reg->friendlyName),
            new IR::DDlogTypeString(),
            new IR::ParameterList({
                new IR::Parameter(IR::ID("ismatch"),
                                  IR::Direction::None,
                                  IR::Type_Boolean::get())}),
            new IR::DDlogIfExpression(
                new IR::DDlogVarName("ismatch"),
                new IR::DDlogStringLiteral(reg->asDDlogString(true)),
                new IR::DDlogStringLiteral(reg->asDDlogString(false))));
        ddlog->push_back(ddfunc);
    }
    return reg;
}

class ResourceAllocator : public Inspector {
    OFResources& resources;
    IR::Vector<IR::Node>* ddlog;
 public:
    ResourceAllocator(OFResources& resources, IR::Vector<IR::Node>* ddlog):
            resources(resources), ddlog(ddlog) { visitDagOnce = false; }
    bool preorder(const IR::Declaration_Variable* decl) {
        (void)allocateRegister(decl, resources, ddlog);
        return false;
    }
};

OFP4Program::OFP4Program(const IR::P4Program* program, const IR::ToplevelBlock* top,
                P4::ReferenceMap* refMap, P4::TypeMap* typeMap):
        program(program), top(top), refMap(refMap), typeMap(typeMap), resources(typeMap) {
    CHECK_NULL(refMap); CHECK_NULL(typeMap); CHECK_NULL(top); CHECK_NULL(program);
}

void OFP4Program::addFixedRules(IR::Vector<IR::Node> *declarations) {
    // drop if output port is 0
    auto match = new IR::OF_SeqMatch();
    match->push_back(new IR::OF_TableMatch(egressExitId));
    match->push_back(new IR::OF_EqualsMatch(
                         outputPortRegister,
                         new IR::OF_Constant(0)));
    match->push_back(new IR::OF_PriorityMatch(new IR::OF_Constant(100)));
    auto flowRule = new IR::OF_MatchAndAction(match, new IR::OF_DropAction());
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
    match = new IR::OF_SeqMatch();
    match->push_back(new IR::OF_TableMatch(multicastId));
    match->push_back(new IR::OF_EqualsMatch(multicastRegister, new IR::OF_Constant(0)));
    flowRule = new IR::OF_MatchAndAction(
        match,
        new IR::OF_ResubmitAction(egressStartId));
    declarations->push_back(makeFlowRule(flowRule, "if multicast group is 0 just forward"));
    // - multicast group non-zero: clone packet for each row from the MuticastGroup table
    match = new IR::OF_SeqMatch();
    match->push_back(new IR::OF_TableMatch(multicastId));
    match->push_back(new IR::OF_EqualsMatch(multicastRegister,
                                            new IR::OF_InterpolatedVarExpression("mcast_id", multicastRegister->size)));
    flowRule = new IR::OF_MatchAndAction(
        match,
        new IR::OF_InterpolatedVariableAction("outputs"));
    auto lhs = makeFlowAtom(flowRule);

    auto lookupGroup = new IR::DDlogAtom(
        "MulticastGroup", new IR::DDlogTupleExpression(
            {new IR::DDlogVarName("mcast_id"), new IR::DDlogVarName("port")}));

    auto clone = new IR::OF_CloneAction(
        new IR::OF_SeqAction(
            new IR::OF_LoadAction(
                new IR::OF_InterpolatedVarExpression("port", 16),
                outputPortRegister),
            new IR::OF_ResubmitAction(egressStartId)));
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
                    new IR::DDlogStringLiteral(OpenFlowPrint::toString(clone)),
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

void OFP4Program::build() {
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

IR::DDlogProgram* OFP4Program::convert() {
    // Collect here the DDlog program
    auto decls = new IR::Vector<IR::Node>();

    decls->push_back(new IR::DDlogImport(IR::ID("ofp4lib")));

    for (auto sf : output_metadata_t->fields) {
        auto reg = allocateRegister(sf, resources, decls);
        if (sf->name == "out_port")
            outputPortRegister = reg;
    }
    for (auto sf : ingress_to_arch_t->fields) {
        auto reg = allocateRegister(sf, resources, decls);
        if (sf->name == "out_group")
            multicastRegister = reg;
    }
    for (auto sf : M->fields) {
        (void)allocateRegister(sf, resources, decls);
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
    rgen.generate(ingress_cfg, ingressExitId);
    rgen.generate(egress_cfg, egressExitId);
    addFixedRules(decls);

    auto result = new IR::DDlogProgram(decls);
    return result;
}

void BackEnd::run(OFP4Options& options, const IR::P4Program* program) {
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
    OFP4Program ofp(program, top, refMap, typeMap);
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

}  // namespace OFP4
