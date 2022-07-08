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

#include "frontends/common/constantFolding.h"
#include "frontends/common/resolveReferences/resolveReferences.h"
#include "frontends/p4/typeChecking/typeChecker.h"
#include "frontends/p4/evaluator/evaluator.h"
#include "frontends/p4/fromv1.0/v1model.h"
#include "frontends/p4/moveDeclarations.h"
#include "frontends/p4/simplify.h"
#include "frontends/p4/strengthReduction.h"
#include "frontends/p4/toP4/toP4.h"
#include "frontends/p4/typeMap.h"
#include "frontends/p4/unusedDeclarations.h"
#include "midend.h"
#include "midend/actionSynthesis.h"
#include "midend/compileTimeOps.h"
#include "midend/complexComparison.h"
#include "midend/copyStructures.h"
#include "midend/eliminateTuples.h"
#include "midend/eliminateNewtype.h"
#include "midend/eliminateSerEnums.h"
#include "midend/eliminateSwitch.h"
#include "midend/flattenHeaders.h"
#include "midend/flattenInterfaceStructs.h"
#include "midend/hsIndexSimplify.h"
#include "midend/expandEmit.h"
#include "midend/expandLookahead.h"
#include "midend/global_copyprop.h"
#include "midend/local_copyprop.h"
#include "midend/midEndLast.h"
#include "midend/nestedStructs.h"
#include "midend/noMatch.h"
#include "midend/predication.h"
#include "midend/removeMiss.h"
#include "midend/simplifyKey.h"
#include "midend/tableHit.h"
#include "midend/removeAssertAssume.h"
#include "lower.h"

namespace OFP4 {

MidEnd::MidEnd(OFP4Options& options) {
    setName("MidEnd");
    addPasses({
        options.ndebug ? new P4::RemoveAssertAssume(&refMap, &typeMap) : nullptr,
        new P4::RemoveMiss(&refMap, &typeMap),
        new P4::EliminateNewtype(&refMap, &typeMap),
        new P4::EliminateSerEnums(&refMap, &typeMap),
        new P4::SimplifyKey(&refMap, &typeMap, new P4::IsLikeLeftValue()),
        new P4::ConstantFolding(&refMap, &typeMap),
        new P4::ExpandLookahead(&refMap, &typeMap),
        new P4::ExpandEmit(&refMap, &typeMap),
        new P4::HandleNoMatch(&refMap),
        new P4::StrengthReduction(&refMap, &typeMap),
        new P4::EliminateTuples(&refMap, &typeMap),
        new P4::SimplifyComparisons(&refMap, &typeMap),
        new P4::CopyStructures(&refMap, &typeMap, false),
        new P4::NestedStructs(&refMap, &typeMap),
        new P4::FlattenHeaders(&refMap, &typeMap),
        new P4::FlattenInterfaceStructs(&refMap, &typeMap),
        new P4::Predication(&refMap),
        new P4::MoveDeclarations(),
        new P4::ConstantFolding(&refMap, &typeMap),
        new P4::GlobalCopyPropagation(&refMap, &typeMap),
        new PassRepeated({
            new P4::LocalCopyPropagation(&refMap, &typeMap),
            new P4::ConstantFolding(&refMap, &typeMap),
        }),
        new P4::StrengthReduction(&refMap, &typeMap),
        new P4::MoveDeclarations(),
        new P4::SimplifyControlFlow(&refMap, &typeMap),
        new P4::CompileTimeOperations(),
        new P4::TableHit(&refMap, &typeMap),
        new P4::EliminateSwitch(&refMap, &typeMap),
        new P4::HSIndexSimplifier(&refMap, &typeMap),
        new OFP4::Lower(&refMap, &typeMap),
        new P4::SynthesizeActions(&refMap, &typeMap),
        new P4::MoveActionsToTables(&refMap, &typeMap),
        new P4::SimplifyControlFlow(&refMap, &typeMap),
        new P4::MidEndLast()
    });
    if (options.excludeMidendPasses) {
        removePasses(options.passesToExcludeMidend);
    }
}

}  // namespace OFP4
