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

#include "lower.h"

namespace OFP4 {

const IR::Node* RemoveBooleanValues::postorder(IR::AssignmentStatement* statement) {
    auto type = typeMap->getType(statement->right, true);
    if (!type->is<IR::Type_Boolean>())
        return statement;
    if (statement->right->is<IR::Operation_Binary>() ||
        statement->right->is<IR::Operation_Unary>()) {
        auto t = new IR::AssignmentStatement(
            statement->srcInfo, statement->left, new IR::BoolLiteral(true));
        auto f = new IR::AssignmentStatement(
            statement->srcInfo, statement->left, new IR::BoolLiteral(false));
        auto ifs = new IR::IfStatement(statement->srcInfo, statement->right, t, f);
        return ifs;
    }
    return statement;
}

const IR::PathExpression*
LowerExpressions::createTemporary(const IR::Expression* expression) {
    auto type = typeMap->getType(expression, true);
    auto name = refMap->newName("tmp");
    auto decl = new IR::Declaration_Variable(IR::ID(name), type->getP4Type());
    newDecls.push_back(decl);
    typeMap->setType(decl, type);
    auto assign = new IR::AssignmentStatement(
        expression->srcInfo, new IR::PathExpression(name), expression);
    assignments.push_back(assign);
    return new IR::PathExpression(expression->srcInfo, new IR::Path(name));
}

const IR::Node* LowerExpressions::postorder(IR::Expression* expression) {
    // Just update the typeMap incrementally.
    auto type = typeMap->getType(getOriginal(), true);
    typeMap->setType(expression, type);
    return expression;
}

const IR::Node* LowerExpressions::postorder(IR::P4Control* control) {
    if (newDecls.size() != 0) {
        // prepend declarations
        newDecls.append(control->controlLocals);
        control->controlLocals = newDecls;
        newDecls.clear();
    }
    return control;
}

const IR::Node* LowerExpressions::postorder(IR::Operation_Relation* expression) {
    if (findContext<IR::AssignmentStatement>() ||  // Do not simplify if inside an if condition...
        (expression->is<IR::Neq>() && findContext<IR::Expression>())) {
        // ... except if the condition is complex.
        auto type = typeMap->getType(getOriginal(), true);
        auto name = refMap->newName("tmp");
        auto decl = new IR::Declaration_Variable(IR::ID(name), type->getP4Type());
        newDecls.push_back(decl);
        typeMap->setType(decl, type);
        auto t = new IR::AssignmentStatement(
            expression->srcInfo, new IR::PathExpression(name), new IR::BoolLiteral(true));
        auto f = new IR::AssignmentStatement(
            expression->srcInfo, new IR::PathExpression(name), new IR::BoolLiteral(false));
        if (expression->is<IR::Neq>()) {
            auto eq = new IR::Equ(expression->srcInfo, expression->left, expression->right);
            // swap t and f
            auto ifs = new IR::IfStatement(expression->srcInfo, eq, f, t);
            assignments.push_back(ifs);
        } else {
            auto ifs = new IR::IfStatement(expression->srcInfo, expression, t, f);
            assignments.push_back(ifs);
        }
        auto result = new IR::PathExpression(expression->srcInfo, new IR::Path(name));
        typeMap->setType(result, type->getP4Type());
        return result;
    }
    return expression;
}

const IR::Node* LowerExpressions::postorder(IR::LNot* expression) {
    auto name = refMap->newName("tmp");
    auto type = typeMap->getType(getOriginal(), true);
    auto decl = new IR::Declaration_Variable(IR::ID(name), type->getP4Type());
    newDecls.push_back(decl);
    typeMap->setType(decl, type);
    auto t = new IR::AssignmentStatement(
        expression->srcInfo, new IR::PathExpression(name), new IR::BoolLiteral(true));
    auto f = new IR::AssignmentStatement(
        expression->srcInfo, new IR::PathExpression(name), new IR::BoolLiteral(false));
    // swap t and f
    auto ifs = new IR::IfStatement(expression->srcInfo, expression->expr, f, t);
    assignments.push_back(ifs);
    auto result = new IR::PathExpression(expression->srcInfo, new IR::Path(name));
    typeMap->setType(result, type->getP4Type());
    return result;
}

const IR::Node* LowerExpressions::postorder(IR::Statement* statement) {
    // Insert before a statement whatever temporary assignments were generated
    if (assignments.empty())
        return statement;
    auto block = new IR::BlockStatement(assignments);
    block->push_back(statement);
    assignments.clear();
    return block;
}

}  // namespace OFP4
