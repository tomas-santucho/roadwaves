// RoadWaves SCS Telemetry SDK 1.15 plugin.
// The plugin is deliberately small and non-blocking: it forwards only the truck
// world placement to a loopback UDP receiver. It never opens a public listener.
#include "scssdk_telemetry.h"
#include "common/scssdk_telemetry_truck_common_channels.h"
#include "eurotrucks2/scssdk_eut2.h"

#include <cstdio>
#include <cstring>
#include <atomic>
#include <chrono>

#ifdef _WIN32
#  include <winsock2.h>
#  include <ws2tcpip.h>
#  if defined(_MSC_VER)
#    pragma comment(lib, "ws2_32.lib")
#  endif
using rw_socket_t = SOCKET;
static constexpr rw_socket_t RW_INVALID_SOCKET = INVALID_SOCKET;
#else
#  include <arpa/inet.h>
#  include <sys/socket.h>
#  include <unistd.h>
using rw_socket_t = int;
static constexpr rw_socket_t RW_INVALID_SOCKET = -1;
#endif

#if defined(_WIN32)
#  define RW_EXPORT extern "C" __declspec(dllexport)
#else
#  define RW_EXPORT extern "C" __attribute__((visibility("default")))
#endif

static scs_log_t rw_game_log = nullptr;
static rw_socket_t rw_socket = RW_INVALID_SOCKET;
static std::atomic<unsigned long long> rw_sequence{0};
static char rw_session_id[64] = "roadwaves-scs-uninitialized";

static void rw_start_session() {
    const auto timestamp = std::chrono::duration_cast<std::chrono::microseconds>(
        std::chrono::steady_clock::now().time_since_epoch()).count();
#ifdef _WIN32
    const auto process_id = static_cast<unsigned long long>(GetCurrentProcessId());
#else
    const auto process_id = static_cast<unsigned long long>(getpid());
#endif
    std::snprintf(
        rw_session_id, sizeof(rw_session_id), "roadwaves-scs-%llu-%llu",
        static_cast<unsigned long long>(timestamp), process_id);
    rw_sequence.store(0, std::memory_order_relaxed);
}

static void rw_close_socket() {
    if (rw_socket == RW_INVALID_SOCKET) return;
#ifdef _WIN32
    closesocket(rw_socket);
    WSACleanup();
#else
    close(rw_socket);
#endif
    rw_socket = RW_INVALID_SOCKET;
}

static bool rw_open_socket() {
    if (rw_socket != RW_INVALID_SOCKET) return true;
#ifdef _WIN32
    WSADATA data{};
    if (WSAStartup(MAKEWORD(2, 2), &data) != 0) return false;
#endif
    rw_socket = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
    if (rw_socket == RW_INVALID_SOCKET) {
#ifdef _WIN32
        WSACleanup();
#endif
        return false;
    }
    return true;
}

static void rw_position_callback(const scs_string_t, const scs_u32_t, const scs_value_t *value, const scs_context_t) {
    if (!value || value->type != SCS_VALUE_TYPE_dplacement || !rw_open_socket()) return;
    const auto &position = value->value_dplacement.position;
    char payload[512];
    const auto timestamp = std::chrono::duration_cast<std::chrono::milliseconds>(
        std::chrono::system_clock::now().time_since_epoch()).count();
    const auto sequence = rw_sequence.fetch_add(1, std::memory_order_relaxed);
    const int length = std::snprintf(payload, sizeof(payload),
        "{\"protocol_version\":1,\"game\":\"ets2\",\"session_id\":\"%s\",\"sequence\":%llu,\"timestamp_unix_ms\":%lld,\"world\":{\"x\":%.6f,\"y\":%.6f,\"z\":%.6f},\"heading\":null,\"paused\":false}\n",
        rw_session_id, sequence, static_cast<long long>(timestamp), position.x, position.y, position.z);
    if (length <= 0 || length >= static_cast<int>(sizeof(payload))) return;

    sockaddr_in destination{};
    destination.sin_family = AF_INET;
    destination.sin_port = htons(47927);
    if (inet_pton(AF_INET, "127.0.0.1", &destination.sin_addr) != 1) return;
    sendto(rw_socket, payload, length, 0, reinterpret_cast<const sockaddr *>(&destination), sizeof(destination));
}

RW_EXPORT scs_result_t scs_telemetry_init(const scs_u32_t version, const scs_telemetry_init_params_t *params) {
    if (version != SCS_TELEMETRY_VERSION_CURRENT || !params) return SCS_RESULT_unsupported;
    rw_start_session();
    auto *init = reinterpret_cast<const scs_telemetry_init_params_v101_t *>(params);
    rw_game_log = init->common.log;
    const scs_result_t result = init->register_for_channel(
        SCS_TELEMETRY_TRUCK_CHANNEL_world_placement,
        SCS_U32_NIL,
        SCS_VALUE_TYPE_dplacement,
        SCS_TELEMETRY_CHANNEL_FLAG_each_frame,
        rw_position_callback,
        nullptr);
    if (result != SCS_RESULT_ok && rw_game_log) rw_game_log(SCS_LOG_TYPE_warning, "RoadWaves could not register truck.world.placement");
    if (result == SCS_RESULT_ok && rw_game_log) rw_game_log(SCS_LOG_TYPE_message, "RoadWaves telemetry plugin initialized");
    return result;
}

RW_EXPORT void scs_telemetry_shutdown() {
    rw_game_log = nullptr;
    rw_close_socket();
}
