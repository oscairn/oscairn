// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include <kick_clap/Host.h>

namespace oscairn
{
// RFC 0021 slice 3: an event going into a shared engine for its context, on the context's channel.
inline kick::clap::Event onChannel(kick::clap::Event e,int16_t channel) noexcept
{
    auto* h=reinterpret_cast<clap_event_header_t*>(e.bytes.data());
    if(channel<0 || h->space_id!=CLAP_CORE_EVENT_SPACE_ID)return e;
    if((h->type==CLAP_EVENT_NOTE_ON || h->type==CLAP_EVENT_NOTE_OFF || h->type==CLAP_EVENT_NOTE_CHOKE) && h->size==sizeof(clap_event_note_t))reinterpret_cast<clap_event_note_t*>(h)->channel=channel;
    else if(h->type==CLAP_EVENT_NOTE_EXPRESSION && h->size==sizeof(clap_event_note_expression_t))reinterpret_cast<clap_event_note_expression_t*>(h)->channel=channel;
    else if(h->type==CLAP_EVENT_PARAM_VALUE && h->size==sizeof(clap_event_param_value_t))reinterpret_cast<clap_event_param_value_t*>(h)->channel=channel;
    else if(h->type==CLAP_EVENT_PARAM_MOD && h->size==sizeof(clap_event_param_mod_t))reinterpret_cast<clap_event_param_mod_t*>(h)->channel=channel;
    else if(h->type==CLAP_EVENT_MIDI && h->size==sizeof(clap_event_midi_t)){auto& m=*reinterpret_cast<clap_event_midi_t*>(h);if(m.data[0]>=0x80 && m.data[0]<0xf0)m.data[0]=uint8_t((m.data[0]&0xf0)|channel);}
    return e;
}
// The MIDI channel an event addresses, or -1 for none (the beat, a harmony, an event for every channel).
inline int channelOf(const kick::clap::Event& e) noexcept
{
    const auto* h=e.header();
    if(h->space_id!=CLAP_CORE_EVENT_SPACE_ID)return -1;
    if((h->type==CLAP_EVENT_NOTE_ON || h->type==CLAP_EVENT_NOTE_OFF || h->type==CLAP_EVENT_NOTE_CHOKE) && h->size==sizeof(clap_event_note_t))return reinterpret_cast<const clap_event_note_t*>(h)->channel;
    if(h->type==CLAP_EVENT_NOTE_EXPRESSION && h->size==sizeof(clap_event_note_expression_t))return reinterpret_cast<const clap_event_note_expression_t*>(h)->channel;
    if(h->type==CLAP_EVENT_PARAM_VALUE && h->size==sizeof(clap_event_param_value_t))return reinterpret_cast<const clap_event_param_value_t*>(h)->channel;
    if(h->type==CLAP_EVENT_PARAM_MOD && h->size==sizeof(clap_event_param_mod_t))return reinterpret_cast<const clap_event_param_mod_t*>(h)->channel;
    if(h->type==CLAP_EVENT_MIDI && h->size==sizeof(clap_event_midi_t)){const auto s=reinterpret_cast<const clap_event_midi_t*>(h)->data[0];return s>=0x80 && s<0xf0?s&15:-1;}
    return -1;
}
}
