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

#ifndef _EXTENSIONS_OFP4_RESOURCES_H_
#define _EXTENSIONS_OFP4_RESOURCES_H_

#include "frontends/p4/typeMap.h"

/// Model resources of open-flow.

namespace P4OF {

/// This class represents a subrange of an OF register
struct OFRegister {
    static const size_t maxRegister = 32;  // maximum register number
    static const size_t registerSize = 32;  // size of a register in bits

    size_t number;  // Register number
    size_t low;     // Low bit number
    size_t high;    // High bit number

    cstring toString() const {
        cstring result = cstring("reg") + Util::toString(number);
        if (high != registerSize)
            result += "[" + Util::toString(low) + ".." + Util::toString(high) + "]";
        return result;
    }
};

/// This class reprents the OF resources used by a P4 program
class OFResources {
    P4::TypeMap* typeMap;
    std::map<const IR::IDeclaration*, OFRegister*> map;
    std::map<const IR::P4Table*, size_t> tableId;
    size_t       currentRegister = 0;  // current free register

 public:
    explicit OFResources(P4::TypeMap* typeMap): typeMap(typeMap)
    { CHECK_NULL(typeMap); }

    OFRegister* allocateRegister(const IR::IDeclaration* decl) {
        auto node = decl->getNode();
        auto type = typeMap->getType(node, true);
        size_t width = typeMap->widthBits(type, node, true);
        size_t min_width = typeMap->widthBits(type, node, false);
        if (width != min_width)
            ::error(ErrorType::ERR_INVALID, "%1%: Unsupported type %2%", decl, type);

        if (currentRegister >= OFRegister::maxRegister) {
            ::error(ErrorType::ERR_OVERLIMIT, "Exhausted register space");
            return nullptr;
        }
        if (width > OFRegister::registerSize) {
            ::error(ErrorType::ERR_OVERLIMIT, "%1%: Cannot yet allocate objects with %2% bits",
                    decl, width);
            return nullptr;
        }
        auto result = new OFRegister;
        result->number = currentRegister++;
        result->low = 0;
        result->high = width;
        map.emplace(decl, result);
        LOG3("Allocated " << result->toString() << " for " << decl);
        return result;
    }
    size_t allocateTable(const IR::P4Table* table) {
        size_t id = tableId.size();
        tableId.emplace(table, id);
        return id;
    }
};

}  // namespace P4OF

#endif  /* _EXTENSIONS_OFP4_RESOURCES_H_ */