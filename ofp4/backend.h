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

/// Backend of the p4c-of compiler.

#ifndef _EXTENSIONS_OFP4_BACKEND_H_
#define _EXTENSIONS_OFP4_BACKEND_H_

#include "ir/ir.h"
#include "frontends/common/options.h"
#include "frontends/common/resolveReferences/referenceMap.h"
#include "frontends/p4/typeMap.h"
#include "options.h"
#include "resources.h"
#include "controlFlowGraph.h"

namespace OFP4 {

/// P4 compiler backend for OpenFlow targets.
class BackEnd {
    P4::ReferenceMap* refMap;
    P4::TypeMap*      typeMap;
 public:
    BackEnd(P4::ReferenceMap* refMap, P4::TypeMap* typeMap):
            refMap(refMap), typeMap(typeMap) {}
    void run(OFP4Options& options, const IR::P4Program* program);
};

/// Summary of the structure of a P4 program written for the of_model.p4 target
class OFP4Program {
 public:
    const IR::P4Program* program = nullptr;
    const IR::ToplevelBlock* top = nullptr;
    P4::ReferenceMap*    refMap = nullptr;
    P4::TypeMap*         typeMap = nullptr;
    const IR::P4Control* ingress = nullptr;
    const IR::P4Control* egress = nullptr;

    // These correspond directly to parameters of the Ingress block
    const IR::Parameter* ingress_hdr = nullptr;
    const IR::Parameter* ingress_meta = nullptr;
    const IR::Parameter* ingress_meta_in = nullptr;
    const IR::Parameter* ingress_itoa = nullptr;
    const IR::Parameter* ingress_meta_out = nullptr;

    // These correspond directly to parameters of the Egress block
    const IR::Parameter* egress_hdr = nullptr;
    const IR::Parameter* egress_meta = nullptr;
    const IR::Parameter* egress_meta_in = nullptr;
    const IR::Parameter* egress_meta_out = nullptr;

    // These correspond directly to the types of the parameters of the ingress/egress blocks
    const IR::Type_Struct* Headers = nullptr;  // type of ingress_hdr and egress_hdr
    const IR::Type_Struct* input_metadata_t = nullptr;  // type of ingress_meta_in, egress_meta_in
    const IR::Type_Struct* M = nullptr;  // type of ingress_meta and egress_meta
    const IR::Type_Struct* ingress_to_arch_t = nullptr;  // type of ingress_itoa
    const IR::Type_Struct* output_metadata_t = nullptr;  // type of ingress_meta_out,egress_meta_out

    // These will be used as OF table=ID nodes in the generated code.
    size_t startIngressId;  // CFG node id of the entry point to ingress
    size_t ingressExitId;   // CFG node id of the ingress exit point
    size_t multicastId;     // CFG node id of the built-in multicast stage
    size_t egressStartId;   // CFG node id of the entry point to egress
    size_t egressExitId;    // CFG node id of the exit point of egress
    OFResources resources;
    const IR::OF_Register* outputPortRegister = nullptr;
    const IR::OF_Register* multicastRegister = nullptr;

    CFG ingress_cfg;
    CFG egress_cfg;

    OFP4Program(const IR::P4Program* program, const IR::ToplevelBlock* top,
                P4::ReferenceMap* refMap, P4::TypeMap* typeMap);
    void build();
    void addFixedRules(IR::Vector<IR::Node> *declarations);
    IR::DDlogProgram* convert();
};

}  // namespace OFP4

#endif  /* _EXTENSIONS_OFP4_BACKEND_H_ */
