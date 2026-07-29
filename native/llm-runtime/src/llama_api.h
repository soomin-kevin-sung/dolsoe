#pragma once

#include "llama.h"
#include <memory>
#include <string>

#define LLW_LLAMA_DLL_SYMBOLS(X) \
    X(llama_backend_free) \
    X(llama_backend_init) \
    X(llama_batch_free) \
    X(llama_batch_init) \
    X(llama_context_default_params) \
    X(llama_decode) \
    X(llama_free) \
    X(llama_get_memory) \
    X(llama_init_from_model) \
    X(llama_memory_seq_rm) \
    X(llama_chat_apply_template) \
    X(llama_model_chat_template) \
    X(llama_model_default_params) \
    X(llama_model_free) \
    X(llama_model_get_vocab) \
    X(llama_model_load_from_file) \
    X(llama_sampler_accept) \
    X(llama_sampler_chain_add) \
    X(llama_sampler_chain_default_params) \
    X(llama_sampler_chain_init) \
    X(llama_sampler_free) \
    X(llama_sampler_init_dist) \
    X(llama_sampler_init_greedy) \
    X(llama_sampler_init_grammar) \
    X(llama_sampler_init_min_p) \
    X(llama_sampler_init_penalties) \
    X(llama_sampler_init_temp) \
    X(llama_sampler_init_top_k) \
    X(llama_sampler_init_top_p) \
    X(llama_sampler_sample) \
    X(llama_token_to_piece) \
    X(llama_tokenize) \
    X(llama_vocab_is_eog)

#define LLW_GGML_DLL_SYMBOLS(X) \
    X(ggml_backend_dev_count) \
    X(ggml_backend_dev_get) \
    X(ggml_backend_load_all_from_path)

#define LLW_GGML_BASE_DLL_SYMBOLS(X) \
    X(ggml_backend_dev_backend_reg) \
    X(ggml_backend_dev_get_props) \
    X(ggml_backend_dev_name) \
    X(ggml_backend_dev_type) \
    X(ggml_backend_reg_name)

class LlamaApi final {
public:
    static std::shared_ptr<LlamaApi> load(const std::string& directory);
    ~LlamaApi();

    LlamaApi(const LlamaApi&) = delete;
    LlamaApi& operator=(const LlamaApi&) = delete;

#define LLW_DECLARE_SYMBOL(name) decltype(&::name) name{};
    LLW_LLAMA_DLL_SYMBOLS(LLW_DECLARE_SYMBOL)
    LLW_GGML_DLL_SYMBOLS(LLW_DECLARE_SYMBOL)
    LLW_GGML_BASE_DLL_SYMBOLS(LLW_DECLARE_SYMBOL)
#undef LLW_DECLARE_SYMBOL

private:
    explicit LlamaApi(const std::string& canonical_directory);

    void* llama_module_{};
    void* ggml_module_{};
    void* ggml_base_module_{};
};
