# Tooling for building ofp4.
#
# This will automatically:
#
#  - Rebuild p4c-of when the .cpp or .h files in this directory
#    change.
#
#    To get this to work, set P4C_OF_BUILD to your p4c build
#    directory.  You can do that by creating a file Make.vars and
#    putting a variable assignment in it.  Or you can leave the
#    default, below, if it is correct.
#
#  - Build .dl and .p4info files from .p4 files.
#
#    To allow this to find the right .p4 files, set the P4 variable
#    correctly below.  Keep the list in the P4 variable up-to-date
#    with the list of imports in ofp4dl.dl.
#
#  - Compile the .dl files into Rust with ddlog.

P4C_OF_BUILD = ../../p4c/build
-include Make.vars		# Possibly override P4C_OF_BUILD.

pathsearch = $(firstword $(wildcard $(addsuffix /$(1),$(subst :, ,$(PATH)))))
xpathsearch = $(if $(call pathsearch,$(1)),$(call pathsearch,$(1)),$(error $(1) not found in $$PATH))
DDLOG := $(call xpathsearch,ddlog)
P4C_OF := $(call xpathsearch,p4c-of)

all: ofp4dl_ddlog.stamp

P4 = snvs.p4 tests/wire.p4
DL = $(P4:.p4=.dl)
P4INFO = $(P4:.p4=.p4info.bin) $(P4:.p4=.p4info.json) $(P4:.p4=.p4info.txt)

# This is "-L $(dir)" for the directory part of every filename in $(P4).
P4INCLUDES = $(patsubst %/,-L %,$(filter-out ./,$(sort $(dir $(P4)))))

ofp4dl_ddlog.stamp: ofp4dl.dl $(DL) $(P4INFO) $(DDLOG)
	$(DDLOG) $(P4INCLUDES) -i ofp4dl.dl
	touch $@
%.dl %.p4info.bin %.p4info.json %.p4info.txt: %.p4 $(P4C_OF)
	$(P4C_OF) $< -o $(<:.p4=.dl) --p4runtime-files $(<:.p4=.p4info.bin),$(<:.p4=.p4info.json),$(<:.p4=.p4info.txt)

# Rebuild p4c-of, if we have the build directory, when .cpp or .h
# files change.
ifneq ($(wildcard $(P4C_OF_BUILD)),)
$(P4C_OF_BUILD)/p4c-of: *.cpp *.h
	cd $(P4C_OF_BUILD) && $(MAKE) p4c-of
# The `touch` below is because cmake only does a copy if the installed
# file would change.
$(P4C_OF): $(P4C_OF_BUILD)/p4c-of
	cd $(P4C_OF_BUILD) && $(MAKE) install && touch $@
endif

clean:
	rm -f $(DL) $(P4INFO)
	rm -rf ofp4dl_ddlog ofp4dl_ddlog.stamp
