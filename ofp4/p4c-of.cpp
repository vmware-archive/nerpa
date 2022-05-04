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

/// Entry point for p4c-of compiler: compiler generating code for open-flow

#include <fstream>
#include <iostream>

#include "control-plane/p4RuntimeSerializer.h"
#include "ir/ir.h"
#include "ir/json_loader.h"
#include "lib/log.h"
#include "lib/error.h"
#include "lib/exceptions.h"
#include "lib/gc.h"
#include "lib/crash.h"
#include "lib/nullstream.h"
#include "frontends/common/applyOptionsPragmas.h"
#include "frontends/common/parseInput.h"
#include "frontends/p4/evaluator/evaluator.h"
#include "frontends/p4/frontend.h"
#include "frontends/p4/toP4/toP4.h"
#include "midend.h"
#include "backend.h"
#include "options.h"

using P4OFContext = P4CContextWithOptions<P4OF::P4OFOptions>;

bool done(const IR::P4Program* program) {
    return program == nullptr || ::errorCount() > 0;
}

void compile(P4OF::P4OFOptions& options) {
    auto hook = options.getDebugHook();
    const IR::P4Program * program = P4::parseP4File(options);
    if (done(program))
        return;

    P4::P4COptionPragmaParser optionsPragmaParser;
    program->apply(P4::ApplyOptionsPragmas(optionsPragmaParser));

    P4::FrontEnd fe;
    fe.addDebugHook(hook);
    program = fe.run(options, program);
    if (done(program))
        return;

    P4::serializeP4RuntimeIfRequired(program, options);
    P4OF::MidEnd midend(options);
    midend.addDebugHook(hook);
    program = program->apply(midend);
    if (done(program))
        return;

    P4OF::BackEnd backend(options, &midend.refMap, &midend.typeMap);
    backend.addDebugHook(hook);
    program->apply(backend);
    if (backend.program != nullptr)
        backend.program->emit(options.outputFile);
}

int main(int argc, char *const argv[]) {
    setup_gc_logging();
    setup_signals();

    AutoCompileContext autoP4TestContext(new P4OFContext);
    auto& options = P4OFContext::get().options();
    options.langVersion = CompilerOptions::FrontendVersion::P4_16;
    options.compilerVersion = "0.1";

    if (options.process(argc, argv) != nullptr)
        options.setInputFile();
    if (::errorCount() > 0)
        return 1;

    compile(options);
    if (Log::verbose())
        std::cerr << "Done." << std::endl;
    return ::errorCount() > 0;
}
