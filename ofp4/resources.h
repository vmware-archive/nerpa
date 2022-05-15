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

#include "ir/ir.h"
#include "frontends/p4/typeMap.h"

/// Model resources of open-flow.

namespace P4OF {

/// This class reprents the OF resources used by a P4 program
class OFResources {
    P4::TypeMap* typeMap;
    std::map<const IR::IDeclaration*, const IR::OF_Register*> map;
    size_t       currentRegister = 0;  // current free register

 public:
    explicit OFResources(P4::TypeMap* typeMap): typeMap(typeMap)
    { CHECK_NULL(typeMap); }

    static cstring makeId(cstring name) {
        return name.replace(".", "_");
    }

    const IR::OF_Register* allocateRegister(const IR::IDeclaration* decl) {
        // TODO: this wastes a lot of space in the holes; it just
        // allocates from the next aligned register
        auto node = decl->getNode();
        auto type = typeMap->getType(node, true);
        size_t width = typeMap->widthBits(type, node, true);
        size_t min_width = typeMap->widthBits(type, node, false);
        if (width != min_width)
            ::error(ErrorType::ERR_INVALID, "%1%: Unsupported type %2%", decl, type);

        if (width > IR::OF_Register::registerSize * IR::OF_Register::maxBundleSize) {
            ::error(ErrorType::ERR_OVERLIMIT, "%1%: Cannot allocate objects with %2% bits",
                    decl, width);
            return nullptr;
        }
        size_t bundle = 1;
        while (width > bundle * IR::OF_Register::registerSize) {
            bundle *= 2;
        }
        // align
        currentRegister = ((currentRegister + (bundle - 1)) / bundle) * bundle;
        if (currentRegister + bundle >= IR::OF_Register::maxRegister) {
            ::error(ErrorType::ERR_OVERLIMIT, "Exhausted register space");
            return nullptr;
        }
        auto result = new IR::OF_Register(currentRegister, 0, width, bundle,
                                          makeId(decl->externalName()));
        currentRegister += bundle;
        map.emplace(decl, result);
        LOG3("Allocated " << result->toString() << " for " << decl);
        return result;
    }

    const IR::OF_Register* getRegister(const IR::IDeclaration* decl) const {
        auto result = ::get(map, decl);
        return result;
    }
};

}  // namespace P4OF

#endif  /* _EXTENSIONS_OFP4_RESOURCES_H_ */
