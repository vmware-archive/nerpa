#!/usr/bin/env python3
# Copyright 2022 Vmware, Inc.
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#    http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""Runs an open-flow test on a P4 program"""

from subprocess import Popen
from threading import Thread
import sys
import os

SUCCESS = 0
FAILURE = 1

class Options(object):
    """Compiler options"""
    def __init__(self):
        self.binary = ""                # this program's name
        self.p4filename = ""            # file that is being compiled
        self.compilerSrcDir = ""        # path to compiler source tree
        self.verbose = False
        self.compilerOptions = []

def usage(options):
    name = options.binary
    print(name, "usage:")
    print(name, "rootdir [options] file.p4")
    print("Invokes compiler on the supplied file, possibly adding extra arguments")
    print("`rootdir` is the root directory of the compiler source tree")
    print("options:")
    print("          -v: verbose operation")
    print("          -a \"args\": pass args to the compiler")


class Local(object):
    # object to hold local vars accessable to nested functions
    pass


def run_timeout(options, args, timeout, stderr):
    print(" ".join(args))
    local = Local()
    local.process = None
    local.filter = None

    def target():
        procstderr = None
        local.process = Popen(args, stderr=procstderr)
        local.process.wait()
        if local.filter is not None:
            local.filter.stdin.close()
            local.filter.wait()

    thread = Thread(target=target)
    thread.start()
    thread.join(timeout)
    if thread.is_alive():
        a = " ".join(args)
        print("Timeout " + a, file=sys.stderr)
        local.process.terminate()
        thread.join()
    if local.process is None:
        # never even started
        if options.verbose:
            print("Process failed to start")
        return -1
    if options.verbose:
        print("Exit code ", local.process.returncode)
    return local.process.returncode


timeout = 10 * 60


def process_file(options, argv):
    assert isinstance(options, Options)
    if not os.path.isfile(options.p4filename):
        raise Exception("No such file " + options.p4filename)
    args = ["./p4c-of"] + options.compilerOptions
    args.extend(argv)
    result = run_timeout(options, args, timeout, None)

    if result != SUCCESS:
        print("Error compiling")

    return result


def main(argv):
    options = Options()

    options.binary = argv[0]
    if len(argv) <= 2:
        usage(options)
        sys.exit(FAILURE)

    options.compilerSrcdir = argv[1]
    argv = argv[2:]
    if not os.path.isdir(options.compilerSrcdir):
        print(options.compilerSrcdir + " is not a folder", file=sys.stderr)
        usage(options)
        sys.exit(FAILURE)

    while argv[0][0] == '-':
        if argv[0] == "-v":
            options.verbose = True
        elif argv[0] == "-a":
            if len(argv) == 0:
                print("Missing argument for -a option")
                usage(options)
                sys.exit(FAILURE)
            else:
                options.compilerOptions += argv[1].split()
                argv = argv[1:]
        elif argv[0][1] == 'D' or argv[0][1] == 'I' or argv[0][1] == 'T':
            options.compilerOptions.append(argv[0])
        else:
            print("Uknown option ", argv[0], file=sys.stderr)
            usage(options)
            sys.exit(FAILURE)
        argv = argv[1:]

    options.p4filename = argv[-1]
    options.testName = None
    if options.p4filename.startswith(options.compilerSrcdir):
        options.testName = options.p4filename[len(options.compilerSrcdir):]
        if options.testName.startswith('/'):
            options.testName = options.testName[1:]
        if options.testName.endswith('.p4'):
            options.testName = options.testName[:-3]

    result = process_file(options, argv)
    sys.exit(result)


if __name__ == "__main__":
    main(sys.argv)
