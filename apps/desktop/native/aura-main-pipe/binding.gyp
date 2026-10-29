{
  "targets": [
    {
      "target_name": "aura_main_pipe",
      "sources": ["src/addon.cpp", "src/connection_io.cpp", "src/process_activity.cpp"],
      "defines": [
        "NAPI_VERSION=10",
        "WIN32_LEAN_AND_MEAN",
        "NOMINMAX",
        "UNICODE",
        "_UNICODE",
        "_WIN32_WINNT=0x0A00"
      ],
      "msvs_settings": {
        "VCCLCompilerTool": {
          "AdditionalOptions": ["/std:c++20"],
          "WarningLevel": 4,
          "ExceptionHandling": 1
        },
        "VCLinkerTool": {
          "AdditionalOptions": ["/Brepro"]
        }
      },
      "conditions": [
        ["OS=='win' and target_arch=='x64'", {
          "defines": ["AURA_MAIN_PIPE_WIN32_X64=1"]
        }, {
          "type": "none"
        }]
      ]
    }
  ]
}
