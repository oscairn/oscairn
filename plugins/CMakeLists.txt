# First-party CLAP plugins. Each builds as a loadable .clap module.
function(oscairn_add_plugin target output source)
    add_library(${target} MODULE ${source})
    target_link_libraries(${target} PRIVATE oscairn_clap_headers)
    target_compile_options(${target} PRIVATE -Wall -Wextra)
    set_target_properties(${target} PROPERTIES
        PREFIX ""
        SUFFIX ".clap"
        OUTPUT_NAME ${output}
        CXX_VISIBILITY_PRESET hidden
        VISIBILITY_INLINES_HIDDEN ON)
    # lib/clap is where CLAP hosts look on Linux (/usr/lib/clap).
    install(TARGETS ${target} LIBRARY DESTINATION lib/clap)
endfunction()

oscairn_add_plugin(oscairn_step_sequencer oscairn-step-sequencer step-sequencer/step_sequencer.cpp)
oscairn_add_plugin(oscairn_basic_synth oscairn-basic-synth basic-synth/basic_synth.cpp)
oscairn_add_plugin(oscairn_delay oscairn-delay delay/delay.cpp)
# A modulator with no sound: sends on a param-target lane (RFC 0002) to parameters and controls.
oscairn_add_plugin(oscairn_lfo oscairn-lfo lfo/lfo.cpp)
# Split and Join, the nodes at either end of a part's left/right, mid/side and band lanes.
oscairn_add_plugin(oscairn_splitters oscairn-splitters splitters/splitters.cpp)

oscairn_add_plugin(oscairn_transpose oscairn-transpose midi-effects/midi_effect.cpp)
oscairn_add_plugin(oscairn_midi_delay oscairn-midi-delay midi-effects/midi_effect.cpp)
target_compile_definitions(oscairn_midi_delay PRIVATE OSCAIRN_MIDI_DELAY=1)

# The sampler the host's tests use (a stand-in for org.oscairn.sampler, which has its own repository): a sound's SFZ-shaped
# patch names a WAV resolved through the asset store, held in memory and played by a note. Beside the other plugins so the
# app's plugin list finds it; not installed.
add_library(oscairn-test-sampler MODULE ../core/engine/tests/test_sampler.cpp)
target_link_libraries(oscairn-test-sampler PRIVATE oscairn_clap_headers)
set_target_properties(oscairn-test-sampler PROPERTIES PREFIX "" SUFFIX ".clap" CXX_VISIBILITY_PRESET hidden VISIBILITY_INLINES_HIDDEN ON)

# Modular instrument (plugins/modular): downloads JUCE.
if(OSCAIRN_MODULAR OR OSCAIRN_STANDALONE)
    add_subdirectory(modular)
endif()
