/*
Copyright 2022 VMware Inc.

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

#ifndef _EXTENSIONS_OFP4_OPTIONS_H_
#define _EXTENSIONS_OFP4_OPTIONS_H_

#include "frontends/common/options.h"

namespace P4OF {

class P4OFOptions : public CompilerOptions {
 public:
    // file to output to
    cstring outputFile = nullptr;

    P4OFOptions() {
        registerOption("-o", "outfile",
                [this](const char* arg) { outputFile = arg; return true; },
                "Write output to outfile");
    }
};

}  // namespace P4OF

#endif  /* _EXTENSIONS_OFP4_OPTIONS_H_ */
