#!/usr/bin/env bash
set -euo pipefail

BASE_DIR="${HOME}/.local/share/echotype"
BIN_DIR="${BASE_DIR}/bin"
LIB_DIR="${BASE_DIR}/lib"
MODEL_DIR="${BASE_DIR}/models"
mkdir -p "${BIN_DIR}" "${LIB_DIR}" "${MODEL_DIR}"

echo "=========================================================="
echo "  EchoType-Linux 离线语音引擎与模型安装脚本 (Sherpa-ONNX)"
echo "=========================================================="

# 1. 下载预编译的 Sherpa-ONNX 推理引擎 (~16MB)
SHERPA_VER="1.10.30"
SHERPA_TAR="sherpa-onnx-v${SHERPA_VER}-linux-x64-shared.tar.bz2"
SHERPA_URL="https://github.com/k2-fsa/sherpa-onnx/releases/download/v${SHERPA_VER}/${SHERPA_TAR}"

if [ ! -f "${BIN_DIR}/sherpa-onnx-offline" ]; then
    echo "--> [1/2] 正在下载 Sherpa-ONNX 推理二进制 (${SHERPA_URL}) ..."
    cd "${BASE_DIR}"
    curl -SL -o "${SHERPA_TAR}" "${SHERPA_URL}"
    tar -xjf "${SHERPA_TAR}"
    cp -f "sherpa-onnx-v${SHERPA_VER}-linux-x64-shared/bin/sherpa-onnx-offline" "${BIN_DIR}/"
    cp -f "sherpa-onnx-v${SHERPA_VER}-linux-x64-shared/lib/"* "${LIB_DIR}/"
    rm -rf "sherpa-onnx-v${SHERPA_VER}-linux-x64-shared" "${SHERPA_TAR}"
    echo "✓ Sherpa-ONNX 推理二进制已安装至: ${BIN_DIR}/sherpa-onnx-offline"
else
    echo "✓ Sherpa-ONNX 推理二进制已存在"
fi

# 2. 下载轻量级高精度中文 Paraformer 离线模型 (~74MB)
MODEL_NAME="sherpa-onnx-paraformer-zh-small-2024-03-09"
MODEL_TAR="${MODEL_NAME}.tar.bz2"
MODEL_URL="https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/${MODEL_TAR}"

cd "${MODEL_DIR}"
if [ ! -f "${MODEL_DIR}/tokens.txt" ]; then
    echo "--> [2/2] 正在下载中文离线模型 (${MODEL_URL}) ..."
    curl -SL -o "${MODEL_TAR}" "${MODEL_URL}"
    tar -xjf "${MODEL_TAR}"
    cp -rf "${MODEL_NAME}/"* "${MODEL_DIR}/"
    rm -rf "${MODEL_NAME}" "${MODEL_TAR}"
    echo "✓ 离线语音模型已安装至: ${MODEL_DIR}"
else
    echo "✓ 离线语音模型已存在于: ${MODEL_DIR}"
fi

echo "=========================================================="
echo "全部离线语音组件就绪！"
echo "测试样本音频路径: ${MODEL_DIR}/test_wavs/0.wav"
echo "=========================================================="
