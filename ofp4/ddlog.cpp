/*
Copyright 2022 Vmware, Inc.

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

#include "ir/ir.h"

// Implementation of methods for the DDlog* IR classes

namespace IR {

void DDlogProgram::emit(std::ostream &o) const {
    for (auto d : *declarations) {
        o << d->toString() << std::endl;
    }
    o.flush();
}

cstring DDlogTypeAlt::toString() const {
    cstring result = "";
    bool first = true;
    for (auto alt : alternatives) {
        if (!first)
            result += " | ";
        first = false;
        result += alt->toString();
    }
    return result;
}

static cstring direction_to_string(const IR::Declaration* type, Direction direction) {
    switch (direction) {
        case IR::Direction::None:
            return "";
        case IR::Direction::In:
            return "input ";
        case IR::Direction::Out:
            return "output ";
        default:
            BUG("%1% direction 'inout' unexpected", type);
    }
}

cstring DDlogRelationDirect::toString() const {
    return direction_to_string(this, direction) + "relation " + externalName() + "[" + recordType->toString() + "]";
}

cstring parametersToString(IR::IndexedVector<Parameter> const& parameters) {
    cstring result = "(";
    for (auto p : parameters) {
        if (result != "(")
            result += ", ";
        result += p->name.toString();
        result += ": ";
        result += p->type->toString();
    }
    result += ")";
    return result;
}

cstring DDlogIndex::toString() const {
    cstring result = "index " + externalName() + parametersToString(parameters) + " on " + relation + "(";
    bool first = true;
    for (auto f : formals) {
        if (!first)
            result += ", ";
        first = false;
        result += f;
    }
    result += ")";
    return result;
}

cstring DDlogRelationSugared::toString() const {
    return (direction_to_string(this, direction)
            + "relation " + externalName() + parametersToString(parameters));
}

cstring DDlogTypeStruct::toString() const {
    cstring result = externalName();
    result += "{";
    bool first = true;
    for (auto f : fields) {
        if (!first)
            result += ", ";
        first = false;
        result += f->name.toString();
        result += ": ";
        result += f->type->toString();
    }
    result += "}";
    return result;
}

cstring DDlogAtom::toString() const {
    cstring result = relation.toString();
    result += expression->toString();
    return result;
}

cstring DDlogRule::toString() const {
    cstring result = "";
    if (!comment.isNullOrEmpty())
        result = cstring("// ") + comment + "\n";
    result += lhs->toString();
    if (rhs.size()) {
        result += " :- ";
        bool first = true;
        for (auto term : rhs) {
            if (!first)
                result += ",\n   ";
            first = false;
            result += term->toString();
        }
    }
    result += ".\n";
    return result;
}

cstring DDlogIfExpression::toString() const {
    cstring result = cstring("if (");
    result += condition->toString();
    result += ") ";
    result += left->toString();
    result += " else ";
    result += right->toString();
    return result;
}

cstring DDlogFunction::toString() const {
    cstring result = cstring("function ") + name.name;
    result += "(";
    bool first = true;
    for (auto p : parameters->parameters) {
        if (!first)
            result += ", ";
        first = false;
        result += p->name;
        result += ": ";
        result += p->type->toString();
    }
    result += "): ";
    result += returnType->toString() + " {\n";
    result += body->toString().indent(4);
    result += "\n}";
    return result;
}

cstring DDlogMatchExpression::toString() const {
    cstring result = "match(" + matched->toString() + ") {\n";
    bool first = true;
    for (auto c : cases) {
        if (!first)
            result += ",\n";
        first = false;
        result += "    " + c->toString();
    }
    result += "\n}";
    return result;
}

cstring DDlogTupleExpression::toString() const {
    cstring result = "(";
    bool first = true;
    for (auto c : components) {
        if (!first)
            result += ", ";
        first = false;
        result += c->toString();
    }
    result += ")";
    return result;
}

cstring DDlogApply::toString() const {
    cstring result = left->toString();
    result += "." + function + "(";
    bool first = true;
    for (auto c : arguments) {
        if (!first)
            result += ", ";
        first = false;
        result += c->toString();
    }
    result += ")";
    return result;
}

cstring DDlogConstructorExpression::toString() const {
    cstring result = constructor + "{";
    bool first = true;
    for (auto c : arguments) {
        if (!first)
            result += ", ";
        first = false;
        result += c;
    }
    result += "}";
    return result;
}

}  // namespace IR
