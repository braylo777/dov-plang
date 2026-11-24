# .dov pLang — common tasks. Requires cargo and a C11 compiler (`cc`).
DOVC   := target/release/dovc
CC     ?= cc
EXAMPLES := $(sort $(wildcard examples/*.dov))
AGENT_EXAMPLES := $(sort $(wildcard examples/agent_*.dov))
TMP    := $(shell mktemp -d 2>/dev/null || echo /tmp/dov-make)

.PHONY: all build test examples tsan bench metrics readme-check

all: build test examples

build:
	cargo build --release --locked

test:
	cargo test --locked

# Run every example and compare its output with examples/expected/NAME.stdout.
examples: build
	@fail=0; for f in $(EXAMPLES); do \
	  n=$$(basename $$f .dov); \
	  if $(DOVC) run $$f > $(TMP)/$$n.out 2>&1 && diff -u examples/expected/$$n.stdout $(TMP)/$$n.out; then \
	    echo "ok   $$f"; else echo "FAIL $$f"; fail=1; fi; \
	done; exit $$fail

# Build the agent examples with ThreadSanitizer; they must give the expected output.
tsan: build
	@fail=0; for f in $(AGENT_EXAMPLES); do \
	  n=$$(basename $$f .dov); d=$(TMP)/tsan-$$n; mkdir -p $$d; \
	  if $(DOVC) emit-c $$f > $$d/prog.c && cp runtime/dov_rt.h runtime/dov_rt.c $$d/ && \
	     $(CC) -std=c11 -g -O1 -fsanitize=thread -o $$d/prog $$d/prog.c $$d/dov_rt.c && \
	     $$d/prog > $$d/out && diff -u examples/expected/$$n.stdout $$d/out; then \
	    echo "tsan ok   $$f"; else echo "tsan FAIL $$f"; fail=1; fi; \
	done; exit $$fail

bench: build
	python3 bench/run.py

metrics: build
	python3 scripts/metrics.py

readme-check:
	python3 scripts/readme_numbers.py --check
