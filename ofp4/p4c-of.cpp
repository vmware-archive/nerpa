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

class P4OFOptions : public CompilerOptions {
 public:
    bool loadIRFromJson = false;
    P4OFOptions() {
        registerOption("--fromJSON", "file",
                       [this](const char* arg) {
                           loadIRFromJson = true;
                           file = arg;
                           return true;
                       },
                       "read previously dumped json instead of P4 source code");
     }
};

using P4OFContext = P4CContextWithOptions<P4OFOptions>;

int main(int argc, char *const argv[]) {
    setup_gc_logging();
    setup_signals();

    AutoCompileContext autoP4TestContext(new P4OFContext);
    auto& options = P4OFContext::get().options();
    options.langVersion = CompilerOptions::FrontendVersion::P4_16;
    options.compilerVersion = "0.1";

    if (options.process(argc, argv) != nullptr) {
        if (options.loadIRFromJson == false)
            options.setInputFile();
    }
    if (::errorCount() > 0)
        return 1;
    const IR::P4Program *program = nullptr;
    auto hook = options.getDebugHook();
    if (options.loadIRFromJson) {
        std::ifstream json(options.file);
        if (json) {
            JSONLoader loader(json);
            const IR::Node* node = nullptr;
            loader >> node;
            if (!(program = node->to<IR::P4Program>()))
                error(ErrorType::ERR_INVALID, "%s is not a P4Program in json format", options.file);
        } else {
            error(ErrorType::ERR_IO, "Can't open %s", options.file); }
    } else {
        program = P4::parseP4File(options);

        if (program != nullptr && ::errorCount() == 0) {
            P4::P4COptionPragmaParser optionsPragmaParser;
            program->apply(P4::ApplyOptionsPragmas(optionsPragmaParser));

            P4::FrontEnd fe;
            fe.addDebugHook(hook);
            program = fe.run(options, program);
        }
    }

    if (program != nullptr && ::errorCount() == 0) {
        P4::serializeP4RuntimeIfRequired(program, options);
        P4OF::MidEnd midEnd(options);
        midEnd.addDebugHook(hook);
        midEnd.process(program);
        if (program) {
            if (options.dumpJsonFile)
                JSONGenerator(*openFile(options.dumpJsonFile, true), true) << program << std::endl;
        }
    }

    if (Log::verbose())
        std::cerr << "Done." << std::endl;
    return ::errorCount() > 0;
}
