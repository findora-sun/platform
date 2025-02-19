#
#
#

all: build_release_goleveldb

export CARGO_NET_GIT_FETCH_WITH_CLI = true
export PROTOC = $(shell which protoc)

export STAKING_INITIAL_VALIDATOR_CONFIG = $(shell pwd)/src/ledger/src/staking/init/staking_config.json
export STAKING_INITIAL_VALIDATOR_CONFIG_DEBUG_ENV = $(shell pwd)/src/ledger/src/staking/init/staking_config_debug_env.json

FIN_DEBUG ?= /tmp/findora
export ENABLE_QUERY_SERVICE = true

EXTERNAL_ADDRESS = ""

ifndef CARGO_TARGET_DIR
	export CARGO_TARGET_DIR=target
endif

$(info ====== Build root is "$(CARGO_TARGET_DIR)" ======)

bin_dir         = bin
lib_dir         = lib
subdirs = $(bin_dir) $(lib_dir)

WASM_PKG = wasm.tar.gz
lib_files = ./$(WASM_PKG)

define pack
	- rm -rf $(1)
	mkdir $(1)
	cd $(1); for i in $(subdirs); do mkdir $$i; done
	cp \
		./${CARGO_TARGET_DIR}/$(2)/$(1)/findorad \
		./${CARGO_TARGET_DIR}/$(2)/$(1)/abcid \
		./${CARGO_TARGET_DIR}/$(2)/$(1)/fn \
		./${CARGO_TARGET_DIR}/$(2)/$(1)/stt \
		./${CARGO_TARGET_DIR}/$(2)/$(1)/staking_cfg_generator \
		$(shell go env GOPATH)/bin/tendermint \
		$(1)/$(bin_dir)/
	cp $(1)/$(bin_dir)/* ~/.cargo/bin/
	cd $(1)/$(bin_dir)/ && findorad pack
	cp -f /tmp/findorad $(1)/$(bin_dir)/
	cp -f /tmp/findorad ~/.cargo/bin/
endef

install: stop_all build_release_goleveldb
	cp -f release/bin/* /usr/local/bin/
	bash -x tools/systemd_services/install.sh $(EXTERNAL_ADDRESS)

stop_all:
	- pkill abcid
	- pkill tendermint
	- pkill findorad

# Build for cleveldb
build: tendermint_cleveldb
	cargo build --bins -p abciapp -p finutils
	$(call pack,debug)

# Build for cleveldb
build_release: tendermint_cleveldb
	cargo build --release --bins -p abciapp -p finutils
	$(call pack,release)

# Build for goleveldb
build_goleveldb: tendermint_goleveldb
	cargo build --bins -p abciapp -p finutils
	$(call pack,debug)

# Build for goleveldb
build_release_goleveldb: tendermint_goleveldb
	cargo build --release --bins -p abciapp -p finutils
	$(call pack,release)

# Build for goleveldb
build_release_musl_goleveldb: tendermint_goleveldb
	cargo build --release --bins -p abciapp -p finutils --target=x86_64-unknown-linux-musl
	$(call pack,release,x86_64-unknown-linux-musl)

build_release_debug: tendermint_goleveldb
	cargo build --features="debug_env" --release --bins -p abciapp -p finutils
	$(call pack,release)

tendermint_cleveldb:
	- rm $(shell which tendermint)
	bash tools/download_tendermint.sh 'tools/tendermint'
	mkdir -p $(shell go env GOPATH)/bin
	cd tools/tendermint \
		&& $(MAKE) build TENDERMINT_BUILD_OPTIONS=cleveldb \
		&& cp build/tendermint $(shell go env GOPATH)/bin/

tendermint_goleveldb:
	- rm $(shell which tendermint)
	bash tools/download_tendermint.sh 'tools/tendermint'
	cd tools/tendermint && $(MAKE) install

test:
	cargo test --release --workspace -- --test-threads=1 # --nocapture

coverage:
	cargo tarpaulin --timeout=900 --branch --workspace --release \
		|| cargo install cargo-tarpaulin \
		&& cargo tarpaulin --timeout=900 --branch --workspace --release

staking_cfg:
	bash tools/update_staking_cfg.sh

staking_cfg_debug:
	bash tools/update_staking_cfg_debug.sh

bench:
	cargo bench --workspace

lint:
	cargo clippy --workspace
	cargo clippy --workspace --no-default-features
	cargo clippy --workspace --tests

update:
	cargo update

fmt:
	cargo fmt

fmtall:
	bash ./tools/fmt.sh

clean:
	cargo clean
	rm -rf tools/tendermint .git/modules/tools/tendermint
	rm -rf debug release Cargo.lock

cleanall: clean
	git clean -fdx

wasm:
	cd src/components/wasm && wasm-pack build
	tar -zcpf $(WASM_PKG) src/components/wasm/pkg

debug_env: stop_debug_env build_release_debug
	- rm -rf $(FIN_DEBUG)
	mkdir $(FIN_DEBUG)
	cp tools/debug_env.tar.gz $(FIN_DEBUG)/
	cd $(FIN_DEBUG) && tar -xpf debug_env.tar.gz && mv debug_env devnet
	fn setup -S 'http://localhost'
	./tools/devnet/startnodes.sh

run_staking_demo: stop_debug_env
	bash tools/staking/demo.sh

start_debug_env:
	bash ./tools/devnet/startnodes.sh

stop_debug_env:
	bash ./tools/devnet/stopnodes.sh

join_debug_env: stop_debug_env build_release_debug
	bash tools/node_init.sh debug_env

join_qa01: stop_debug_env build_release_goleveldb
	bash tools/node_init.sh qa01

join_qa02: stop_debug_env build_release_goleveldb
	bash tools/node_init.sh qa02

join_testnet: stop_debug_env build_release_goleveldb
	bash tools/node_init.sh testnet

join_mainnet: stop_debug_env build_release_goleveldb
	bash tools/node_init.sh mainnet

start_localnode: stop_debug_env
	bash tools/node_init.sh _ _

# ci_build_image:
# 	@if [ ! -d "release/bin/" ] && [ -d "debug/bin" ]; then \
# 		mkdir -p release/bin/; \
# 		cp debug/bin/findorad release/bin/; \
# 	fi
# 	docker build -t $(ECR_URL)/$(ENV)/abci_validator_node:$(IMAGE_TAG) -f container/Dockerfile-CI-abci_validator_node .
# ifeq ($(ENV),release)
# 	docker tag $(ECR_URL)/$(ENV)/abci_validator_node:$(IMAGE_TAG) $(ECR_URL)/$(ENV)/findorad:latest
# endif

# ci_push_image:
# 	docker push $(ECR_URL)/$(ENV)/abci_validator_node:$(IMAGE_TAG)
# ifeq ($(ENV),release)
# 	docker push $(ECR_URL)/$(ENV)/abci_validator_node:latest
# endif

# clean_image:
# 	docker rmi $(ECR_URL)/$(ENV)/abci_validator_node:$(IMAGE_TAG)
# ifeq ($(ENV),release)
# 	docker rmi $(ECR_URL)/$(ENV)/abci_validator_node:latest
# endif


ci_build_binary_rust_base:
	docker build -t binary-rust-base -f container/Dockerfile-binary-rust-base .

ci_build_dev_binary_image:
	sed -i "s/^ENV VERGEN_SHA_EXTERN .*/ENV VERGEN_SHA_EXTERN ${VERGEN_SHA_EXTERN}/g" container/Dockerfile-binary-image-dev
	docker build -t findorad-binary-image:$(IMAGE_TAG) -f container/Dockerfile-binary-image-dev .

ci_build_release_binary_image:
	sed -i "s/^ENV VERGEN_SHA_EXTERN .*/ENV VERGEN_SHA_EXTERN ${VERGEN_SHA_EXTERN}/g" container/Dockerfile-binary-image-release
	docker build -t findorad-binary-image:$(IMAGE_TAG) -f container/Dockerfile-binary-image-release .

ci_build_image:
	@ if [ -d "./binary" ]; then \
		rm -rf ./binary || true; \
	fi
	@ docker run --rm -d --name findorad-binary findorad-binary-image:$(IMAGE_TAG)
	@ docker cp findorad-binary:/binary ./binary
	@ docker rm -f findorad-binary
	@ docker build -t $(PUBLIC_ECR_URL)/$(ENV)/findorad:$(IMAGE_TAG) -f container/Dockerfile-cleveldb .
ifeq ($(ENV),release)
	docker tag $(PUBLIC_ECR_URL)/$(ENV)/findorad:$(IMAGE_TAG) $(PUBLIC_ECR_URL)/$(ENV)/findorad:latest
endif

ci_push_image:
	docker push $(PUBLIC_ECR_URL)/$(ENV)/findorad:$(IMAGE_TAG)
ifeq ($(ENV),release)
	docker push $(PUBLIC_ECR_URL)/$(ENV)/findorad:latest
endif

clean_image:
	docker rmi $(PUBLIC_ECR_URL)/$(ENV)/findorad:$(IMAGE_TAG)
ifeq ($(ENV),release)
	docker rmi $(PUBLIC_ECR_URL)/$(ENV)/findorad:latest
endif

ci_build_image_dockerhub:
	@ if [ -d "./binary" ]; then \
		rm -rf ./binary || true; \
	fi
	@ docker run --rm -d --name findorad-binary findorad-binary-image:$(IMAGE_TAG)
	@ docker cp findorad-binary:/binary ./binary
	@ docker rm -f findorad-binary
	@ docker build -t $(DOCKERHUB_URL)/findorad:$(IMAGE_TAG) -f container/Dockerfile-goleveldb .
ifeq ($(ENV),release)
	docker tag $(DOCKERHUB_URL)/findorad:$(IMAGE_TAG) $(DOCKERHUB_URL)/findorad:latest
endif

ci_push_image_dockerhub:
	docker push $(DOCKERHUB_URL)/findorad:$(IMAGE_TAG)
ifeq ($(ENV),release)
	docker push $(DOCKERHUB_URL)/findorad:latest
endif

ci_build_wasm_js_bindings:
	docker run --rm -d --name wasm -v /tmp/wasm-js-bindings:/build/wasm-js-bindings -v $(shell pwd)/container/docker-entrypoint-wasm-js-bindings.sh:/entrypoint.sh findorad-binary-image:$(IMAGE_TAG) /entrypoint.sh
	docker rm -f wasm findorad-binary || true

clean_image_dockerhub:
	docker rmi $(DOCKERHUB_URL)/findorad:$(IMAGE_TAG)
ifeq ($(ENV),release)
	docker rmi $(DOCKERHUB_URL)/findorad:latest
endif

clean_binary_dockerhub:
	docker rmi findorad-binary-image:$(IMAGE_TAG)

reset:
	@./tools/devnet/stopnodes.sh
	@./tools/devnet/resetnodes.sh 1 1

snapshot:
	@./tools/devnet/snapshot.sh

devnet: reset snapshot
