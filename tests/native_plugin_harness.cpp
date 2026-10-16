#include "scssdk_telemetry.h"

#include <winsock2.h>
#include <windows.h>

#include <cstring>
#include <iostream>
#include <string>

using init_fn = scs_result_t (*)(scs_u32_t, const scs_telemetry_init_params_t *);
using shutdown_fn = void (*)();

static scs_telemetry_channel_callback_t registered_callback = nullptr;

static scs_result_t register_channel(
    const scs_string_t,
    const scs_u32_t,
    const scs_value_type_t,
    const scs_u32_t,
    const scs_telemetry_channel_callback_t callback,
    const scs_context_t) {
    registered_callback = callback;
    return SCS_RESULT_ok;
}

static scs_result_t unregister_channel(const scs_string_t, const scs_u32_t, const scs_value_type_t) {
    return SCS_RESULT_ok;
}

static void log_message(const scs_log_type_t, const scs_string_t message) {
    if (message) std::cerr << message << "\n";
}

int main(int argc, char **argv) {
    if (argc != 2) {
        std::cerr << "usage: native_plugin_harness <roadwaves_scs.dll>\n";
        return 2;
    }

    WSADATA winsock{};
    if (WSAStartup(MAKEWORD(2, 2), &winsock) != 0) {
        std::cerr << "WSAStartup failed\n";
        return 3;
    }

    SOCKET receiver = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
    if (receiver == INVALID_SOCKET) {
        WSACleanup();
        std::cerr << "receiver socket failed\n";
        return 4;
    }
    sockaddr_in address{};
    address.sin_family = AF_INET;
    address.sin_port = htons(47927);
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (bind(receiver, reinterpret_cast<const sockaddr *>(&address), sizeof(address)) != 0) {
        closesocket(receiver);
        WSACleanup();
        std::cerr << "bind failed; stop any RoadWaves service before running this harness\n";
        return 5;
    }
    DWORD timeout_ms = 1500;
    setsockopt(receiver, SOL_SOCKET, SO_RCVTIMEO, reinterpret_cast<const char *>(&timeout_ms), sizeof(timeout_ms));

    HMODULE module = LoadLibraryA(argv[1]);
    if (!module) {
        closesocket(receiver);
        WSACleanup();
        std::cerr << "LoadLibrary failed\n";
        return 6;
    }
    init_fn init = nullptr;
    shutdown_fn shutdown = nullptr;
    auto init_address = GetProcAddress(module, "scs_telemetry_init");
    auto shutdown_address = GetProcAddress(module, "scs_telemetry_shutdown");
    static_assert(sizeof(init) == sizeof(init_address));
    static_assert(sizeof(shutdown) == sizeof(shutdown_address));
    std::memcpy(&init, &init_address, sizeof(init));
    std::memcpy(&shutdown, &shutdown_address, sizeof(shutdown));
    if (!init || !shutdown) {
        FreeLibrary(module);
        closesocket(receiver);
        WSACleanup();
        std::cerr << "required telemetry exports are missing\n";
        return 7;
    }

    scs_telemetry_init_params_v101_t params{};
    params.common.game_name = "Euro Truck Simulator 2";
    params.common.game_id = "ets2";
    params.common.game_version = 1;
    params.common.log = log_message;
    params.register_for_channel = register_channel;
    params.unregister_from_channel = unregister_channel;
    if (init(SCS_TELEMETRY_VERSION_CURRENT, reinterpret_cast<const scs_telemetry_init_params_t *>(&params)) != SCS_RESULT_ok ||
        !registered_callback) {
        shutdown();
        FreeLibrary(module);
        closesocket(receiver);
        WSACleanup();
        std::cerr << "telemetry initialization or channel registration failed\n";
        return 8;
    }

    scs_value_t value{};
    value.type = SCS_VALUE_TYPE_dplacement;
    value.value_dplacement.position.x = 123.25;
    value.value_dplacement.position.y = 456.5;
    value.value_dplacement.position.z = -78.75;
    registered_callback("truck.world.placement", SCS_U32_NIL, &value, nullptr);

    char payload[1024]{};
    const int received = recv(receiver, payload, sizeof(payload) - 1, 0);
    if (received <= 0) {
        shutdown();
        FreeLibrary(module);
        closesocket(receiver);
        WSACleanup();
        std::cerr << "no telemetry packet received\n";
        return 9;
    }
    payload[received] = '\0';
    const std::string text(payload);
    for (const char *required : {"\"game\":\"ets2\"", "\"session_id\":\"roadwaves-scs-", "\"sequence\":0", "\"x\":123.250000", "\"y\":456.500000", "\"z\":-78.750000"}) {
        if (text.find(required) == std::string::npos) {
            shutdown();
            FreeLibrary(module);
            closesocket(receiver);
            WSACleanup();
            std::cerr << "telemetry packet missing: " << required << "\n" << text;
            return 10;
        }
    }

    const auto session_start = text.find("\"session_id\":\"") + std::strlen("\"session_id\":\"");
    const auto session_end = text.find('"', session_start);
    const std::string first_session = text.substr(session_start, session_end - session_start);
    shutdown();
    registered_callback = nullptr;
    if (init(SCS_TELEMETRY_VERSION_CURRENT, reinterpret_cast<const scs_telemetry_init_params_t *>(&params)) != SCS_RESULT_ok) {
        FreeLibrary(module);
        closesocket(receiver);
        WSACleanup();
        std::cerr << "second telemetry initialization failed\n";
        return 11;
    }
    registered_callback("truck.world.placement", SCS_U32_NIL, &value, nullptr);
    char second_payload[1024]{};
    const int second_received = recv(receiver, second_payload, sizeof(second_payload) - 1, 0);
    second_payload[(second_received > 0) ? second_received : 0] = '\0';
    const std::string second_text(second_payload);
    const auto second_session_start = second_text.find("\"session_id\":\"") + std::strlen("\"session_id\":\"");
    const auto second_session_end = second_text.find('"', second_session_start);
    const std::string second_session = second_text.substr(second_session_start, second_session_end - second_session_start);
    shutdown();
    FreeLibrary(module);
    closesocket(receiver);
    WSACleanup();
    if (second_received <= 0 || second_text.find("\"sequence\":0") == std::string::npos || first_session == second_session) {
        std::cerr << "telemetry session did not restart cleanly\n";
        return 12;
    }
    std::cout << "native_plugin_smoke=passed bytes=" << received << " session_restart=passed\n";
    return 0;
}
