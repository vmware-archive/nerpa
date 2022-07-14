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

#include "lib/algorithm.h"
#include "lib/stringify.h"
#include "ir/ir.h"
#include "frontends/p4/typeMap.h"

/// Model resources of open-flow.

namespace OFP4 {

/// This class reprents the OF resources used by a P4 program
class OFResources {
    P4::TypeMap* typeMap;
    std::map<const IR::IDeclaration*, const IR::OF_Register*> map;
    // One bit for each register byte; if 'true' the byte is allocated.
    std::vector<bool> byteMask;
    size_t bytesPerRegister;

 public:
    explicit OFResources(P4::TypeMap* typeMap): typeMap(typeMap) {
        CHECK_NULL(typeMap);
        bytesPerRegister = IR::OF_Register::registerSize / 8;
        for (size_t i = 0; i < IR::OF_Register::maxRegister * bytesPerRegister; i++)
            byteMask.push_back(false);
    }

    static cstring makeId(cstring name) {
        return name.replace(".", "_");
    }

    const IR::OF_Register* allocateRegister(const IR::IDeclaration* decl) {
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

        cstring name = "";
        size_t size = IR::OF_Register::registerSize;
        while (width > size) {
            size *= 2;
            name += "x";
        }

        bool found = false;
        size_t index;
        size_t bytesNeeded = ROUNDUP(width, 8);
        // Find a gap of bytesNeeded bytes
        for (size_t i = 0; i < byteMask.size() - bytesNeeded + 1; i++) {
            if (!byteMask.at(i)) {
                found = true;
                for (size_t j = i + 1; j < i + bytesNeeded; j++) {
                    if (byteMask.at(j)) {
                        found = false;
                        i = j;
                        break;
                    }
                }
                if (found) {
                    LOG3("Allocating " << bytesNeeded << " at " << i);
                    index = i;
                    break;
                }
            }
        }
        if (!found) {
            ::error(ErrorType::ERR_OVERLIMIT, "Exhausted register space");
            return nullptr;
        }
        for (size_t i = index; i < index + bytesNeeded; i++) {
            assert(!byteMask[i]);
            byteMask[i] = true;
        }
        name += "reg" + Util::toString(index / bytesPerRegister);
        auto result = new IR::OF_Register(name,
                                          size,
                                          (index % bytesPerRegister) * 8,
                                          (index % bytesPerRegister) * 8 + width-1,
                                          makeId(decl->externalName()));
        map.emplace(decl, result);
        LOG3("Allocated " << result->toString() << " for " << decl);
        return result;
    }

    const IR::OF_Register* getRegister(const IR::IDeclaration* decl) const {
        auto result = ::get(map, decl);
        return result;
    }
};

}  // namespace OFP4

#endif  /* _EXTENSIONS_OFP4_RESOURCES_H_ */
