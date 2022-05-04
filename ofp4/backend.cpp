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

namespace P4OF {

void DDlogProgram::emit(cstring file) {
    auto dlStream = openFile(file, false);
    if (dlStream != nullptr) {
        // TODO
        ;
    }
    dlStream->flush();
}

struct OFRegister {
    static const size_t maxRegister = 32;  // maximum register number
    static const size_t registerSize = 32;  // size of a register in bits

    size_t number;  // Register number
    size_t low;     // Low bit number
    size_t high;    // High bit number

    cstring toString() const {
        cstring result = cstring("i\"reg") + Util::toString(number);
        if (high != registerSize)
            result += "[" + Util::toString(low) + ".." + Util::toString(high) + "]";
        return result;
    }
};

/// This class reprents the OF resources used by a P4 program
class Resources {
    std::map<const IR::IDeclaration*, OFRegister*> map;
    std::map<const IR::P4Table*, size_t> tableId;
    size_t       currentRegister = 0;  // current free register

 public:
    OFRegister* allocateRegister(const IR::IDeclaration* decl, size_t bits) {
        if (currentRegister >= OFRegister::maxRegister) {
            ::error(ErrorType::ERR_OVERLIMIT, "Exhausted register space");
            return nullptr;
        }
        if (bits > OFRegister::registerSize) {
            ::error(ErrorType::ERR_OVERLIMIT, "%1%: Cannot yet allocate objects with %2% bits",
                    decl, bits);
            return nullptr;
        }
        auto result = new OFRegister;
        result->number = currentRegister++;
        result->low = 0;
        result->high = bits;
        map.emplace(decl, result);
        return result;
    }
    size_t allocateTable(const IR::P4Table* table) {
        size_t id = tableId.size();
        tableId.emplace(table, id);
        return id;
    }
};

/// Generates DDlog code from a P4 program
class DDlogCodeGenerator : public Inspector {
    P4::ReferenceMap* refMap;
    P4::TypeMap*      typeMap;
    DDlogProgram*     program;
    Resources         resources;
 public:
    DDlogCodeGenerator(P4::ReferenceMap* refMap, P4::TypeMap* typeMap, DDlogProgram* program):
            refMap(refMap), typeMap(typeMap), program(program) {}

    bool preorder(IR::P4Table* table) {
        resources.allocateTable(table);
        return false;
    }
};

/// P4 compiled backend for OpenFlow targets.
BackEnd::BackEnd(P4OFOptions&, P4::ReferenceMap* refMap, P4::TypeMap* typeMap) {
    setName("BackEnd");
    DDlogProgram* program = new DDlogProgram();

    addPasses({
        new DDlogCodeGenerator(refMap, typeMap, program),
    });
}

}  // namespace P4OF
