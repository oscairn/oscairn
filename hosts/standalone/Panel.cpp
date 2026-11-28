// SPDX-License-Identifier: AGPL-3.0-or-later
#include "Panel.h"
#include "Theme.h"

namespace oscairn::standalone
{
    /// JUCE's keyboard, its keys drawn in the theme: faces a little apart on the background,
    /// rounded at the front; the lighter of the text and surface inks softened for the white keys,
    /// so a dark theme's are not stark white, and the darker for the black ones.
    class Keys::Board final : public juce::MidiKeyboardComponent
    {
    public:
        Board( juce::MidiKeyboardState& state, const Keys& keys ) : juce::MidiKeyboardComponent( state, horizontalKeyboard ), keys_( keys )
        {
            setColour( whiteNoteColourId, ink( Ink::Background ) ); // the gaps between the keys
            setColour( shadowColourId, juce::Colours::transparentBlack );
            setBlackNoteLengthProportion( .62f );
        }
        void lookAndFeelChanged() override { setColour( whiteNoteColourId, ink( Ink::Background ) ); repaint(); }

        void drawWhiteNote( int note, juce::Graphics& g, juce::Rectangle<float> area, bool isDown, bool isOver, juce::Colour, juce::Colour ) override
        {
            const auto [light, dark] = inks();
            const auto face = area.reduced( .75f, 0 ).withTrimmedBottom( 1 );
            fill( g, face, dark.interpolatedWith( light, darkTheme() ? .74f : .94f ), 4 ); // on a dark theme, a soft gray, not stark white
            mark( g, note, face, isDown, isOver, 4 );
            if( note % 12 == 0 && getKeyWidth() >= 16 )
            {
                g.setColour( dark.interpolatedWith( light, .35f ) ); // readable on the key in any theme
                g.setFont( juce::Font( juce::FontOptions( juce::jmin( 12.f, getKeyWidth() * .5f ) ) ) );
                g.drawText( getWhiteNoteText( note ), face.withTrimmedBottom( face.getWidth() * .55f + 4 ), juce::Justification::centredBottom, false );
            }
        }
        void drawBlackNote( int note, juce::Graphics& g, juce::Rectangle<float> area, bool isDown, bool isOver, juce::Colour ) override
        {
            const auto [light, dark] = inks();
            auto face = area.reduced( .5f, 0 );
            fill( g, face, dark.interpolatedWith( light, .12f ), 3 );
            g.setColour( dark.interpolatedWith( light, .3f ) ); // the front edge, lit
            g.fillRoundedRectangle( face.removeFromBottom( 3 ).reduced( 1, 0 ), 1.5f );
            mark( g, note, face, isDown, isOver, 3 );
        }

    private:
        static std::pair<juce::Colour, juce::Colour> inks()
        {
            const auto a = ink( Ink::Text ), b = ink( Ink::Surface );
            return a.getPerceivedBrightness() > b.getPerceivedBrightness() ? std::pair{ a, b } : std::pair{ b, a };
        }
        // Square at the back, rounded at the front.
        static void fill( juce::Graphics& g, juce::Rectangle<float> r, juce::Colour c, float radius )
        {
            juce::Path p;
            p.addRoundedRectangle( r.getX(), r.getY(), r.getWidth(), r.getHeight(), radius, radius, false, false, true, true );
            g.setColour( c ); g.fillPath( p );
        }
        // What the key is doing: played here (the accent, filled as high as its velocity),
        // sustained (tinted), hovered, and the input's own note arriving (a dot).
        void mark( juce::Graphics& g, int note, juce::Rectangle<float> face, bool isDown, bool isOver, float radius ) const
        {
            const auto accent = ink( Ink::Accent );
            if( isDown || keys_.sustained( note ) ) fill( g, face, accent.withAlpha( .3f ), radius );
            if( isDown ) fill( g, face.withTrimmedTop( face.getHeight() * ( 1 - juce::jlimit( .08f, 1.f, keys_.velocity( note ) ) ) ), accent, radius );
            else if( isOver ) fill( g, face, ink( Ink::Text ).withAlpha( .07f ), radius );
            if( keys_.incoming( note ) && !isDown && !keys_.sustained( note ) ) // not a key played here, which shows as its own
            {
                const float d = juce::jmin( face.getWidth() * .55f, 14.f );
                const auto dot = juce::Rectangle<float>( d, d ).withCentre( { face.getCentreX(), face.getBottom() - d * .5f - 4 } );
                g.setColour( ink( Ink::Positive ) ); g.fillEllipse( dot );
                g.setColour( ink( Ink::Background ).withAlpha( .6f ) ); g.drawEllipse( dot, 1 );
            }
        }
        const Keys& keys_;
    };

    Keys::Keys( Engine& engine, int lowest, int count ) : engine_( engine ), keyboard_( std::make_unique<Board>( state_, *this ) )
    {
        state_.addListener( this );
        keyboard_->setOctaveForMiddleC( 4 );
        show( lowest, count );
        setTitle( "Keyboard" );
        addAndMakeVisible( *keyboard_ );
    }
    Keys::~Keys() { state_.removeListener( this ); }

    int Keys::lowest() const { return keyboard_->getRangeStart(); }
    int Keys::highest() const { return keyboard_->getRangeEnd(); }
    juce::MidiKeyboardComponent& Keys::keyboard() { return *keyboard_; }

    void Keys::show( int lowest, int count )
    {
        if( lowest == keyboard_->getRangeStart() && juce::jmin( 127, lowest + count - 1 ) == keyboard_->getRangeEnd() ) return;
        keyboard_->setAvailableRange( lowest, juce::jmin( 127, lowest + count - 1 ) );
        keyboard_->setLowestVisibleKey( lowest );
        keyboard_->setKeyPressBaseOctave( lowest / 12 );
        resized();
    }

    void Keys::press( int note, bool on, float velocity )
    {
        if( on ) state_.noteOn( 1, note, velocity );
        else state_.noteOff( 1, note, 0.0f );
    }

    void Keys::letGo()
    {
        keyboard_->focusLost( juce::Component::focusChangedDirectly ); // its mouse and fingers let go
        state_.allNotesOff( 0 );
    }

    void Keys::release()
    {
        letGo();
        setSustain( false );
    }

    void Keys::setSustain( bool on )
    {
        sustain_ = on;
        if( on ) return;
        for( int note = 0; note < 128; ++note )
            if( sustained_[size_t( note )] && !down( note ) ) { sustained_.reset( size_t( note ) ); send( false, note, 0 ); }
        keyboard_->repaint();
    }

    void Keys::setIncoming( const std::vector<uint8_t>& keys )
    {
        std::bitset<128> now;
        for( const auto k : keys ) if( k < 128 ) now.set( k );
        if( now == incoming_ ) return;
        incoming_ = now;
        keyboard_->repaint();
    }

    void Keys::resized()
    {
        keyboard_->setBounds( getLocalBounds() );
        int whites = 0;
        for( int n = keyboard_->getRangeStart(); n <= keyboard_->getRangeEnd(); ++n )
            whites += juce::MidiMessage::isMidiNoteBlack( n ) ? 0 : 1;
        keyboard_->setKeyWidth( getWidth() / (float)juce::jmax( 1, whites ) );
    }

    void Keys::send( bool on, int note, float velocity ) { engine_.note( on, note, velocity, channel ? channel() : -1, input ); }

    // A key at its very top has no velocity: it still sounds, softly. Played again while
    // sustained, it is let go first, as a piano's damper lifts.
    void Keys::handleNoteOn( juce::MidiKeyboardState*, int, int note, float velocity )
    {
        if( sustained_[size_t( note )] ) { sustained_.reset( size_t( note ) ); send( false, note, 0 ); }
        velocity_[size_t( note )] = juce::jmax( velocity, .01f );
        send( true, note, velocity_[size_t( note )] );
    }
    void Keys::handleNoteOff( juce::MidiKeyboardState*, int, int note, float )
    {
        if( sustain_ ) { sustained_.set( size_t( note ) ); keyboard_->repaint(); return; }
        send( false, note, 0.0f );
    }

    /// The panel at its design size; Panel scales it to fit.
    class Panel::Board : public juce::Component
    {
    public:
        explicit Board( const Layout& layout ) : layout_( layout )
        {
            setSize( layout.width, layout.height );
            for( const auto& c : layout.controls )
            {
                using Type = Layout::Control::Type;
                std::unique_ptr<juce::Component> component;
                switch( c.type )
                {
                case Type::Screen:
                    // The area Home and the apps cover.
                    component = std::make_unique<juce::Component>();
                    screen_   = component.get();
                    break;
                case Type::Label: break; // drawn by paint()
                }
                if( component )
                {
                    addAndMakeVisible( *component );
                    placed_.emplace_back( component.get(), &c );
                    owned_.push_back( std::move( component ) );
                }
            }
            layOut( 0, 0 );
        }

        /// Sizes the board to the design plus `dx`, `dy` extra units, moving and sizing
        /// every element by its anchor.
        void layOut( int dx, int dy )
        {
            dx_ = dx;
            dy_ = dy;
            setSize( layout_.width + dx, layout_.height + dy );
            for( auto& [component, control] : placed_ )
                component->setBounds( control->anchor.place( control->rect, dx, dy ) );
            if( overlay_ ) overlay_->setBounds( screenBounds() );
            repaint();
        }
        juce::Rectangle<int> screenBounds() const { return screen_ ? screen_->getBounds() : getLocalBounds(); }
        juce::Component* overlay_ = nullptr; // a view over the screen

        void paint( juce::Graphics& g ) override
        {
            g.fillAll( ink(Ink::Background) );
            for( const auto& p : layout_.panels )
            {
                const auto r = p.anchor.place( p.rect, dx_, dy_ );
                g.setColour( ink(Ink::Raised).darker( 0.3f ) );
                g.fillRoundedRectangle( r.toFloat(), themeData().radius );
                g.setColour( ink(Ink::Muted) );
                g.setFont( juce::FontOptions( 13.0f*themeData().fontScale, juce::Font::bold ) );
                g.drawText( p.label, r.withHeight( 20 ), juce::Justification::centred );
            }
            g.setColour( ink(Ink::Text) );
            for( const auto& c : layout_.controls )
                if( c.type == Layout::Control::Type::Label )
                {
                    g.setFont( juce::FontOptions( (float)c.rect.getHeight() * 0.7f, juce::Font::bold ) );
                    g.drawText( c.label, c.anchor.place( c.rect, dx_, dy_ ), c.align );
                }
        }



    private:
        const Layout&                                 layout_;
        std::vector<std::unique_ptr<juce::Component>> owned_;
        std::vector<std::pair<juce::Component*, const Layout::Control*>> placed_;
        int                                           dx_ = 0, dy_ = 0;
        juce::Component*                              screen_ = nullptr;
    };

    Panel::Panel( Engine& engine, const Layout& layout ) : layout_( layout ),engine_(engine)
    {
        board_ = std::make_unique<Board>( layout_ );
        addAndMakeVisible( *board_ );
        addChildComponent(recovery_);
        startTimerHz( 30 );
    }

    Panel::~Panel() = default;

    void Panel::overlay( juce::Component* view )
    {
        if( overlay_ && overlay_ != view ) board_->removeChildComponent( overlay_ );
        overlay_ = board_->overlay_ = view;
        if( view ) { board_->addAndMakeVisible( view ); view->setBounds( board_->screenBounds() ); }
    }

    void Panel::setDesignSize( int width, int height )
    {
        designWidth_ = std::max( 1, width ); designHeight_ = std::max( 1, height );
        resized();
    }

    void Panel::paint( juce::Graphics& g )
    {
        g.fillAll( ink(Ink::Background).darker( 0.4f ) );
    }

    void Panel::resized()
    {
        recovery_.setBounds(dense(*this) ? getLocalBounds().removeFromTop(44).reduced(denseGap, 0) : getLocalBounds().removeFromTop(44).reduced(4)); // dense: a touch target tall, Restart in it
        // Scale the board to fit the tighter dimension. A stretching layout is laid out at its
        // design size and takes the space left in the other, which may be less than the layout's
        // own size: its elements move and stretch by their anchors. Otherwise the board is centred.
        const bool stretch = layout_.resize == Layout::Resize::Stretch;
        const auto scale = stretch ? juce::jmin( getWidth() / (float)designWidth_, getHeight() / (float)designHeight_ )
                                   : juce::jmin( getWidth() / (float)layout_.width, getHeight() / (float)layout_.height );
        if( scale <= 0.0f )
            return;
        if( stretch )
        {
            // Rounded up, so a fractional scale leaves no strip unfilled.
            board_->layOut( (int)std::ceil( getWidth() / scale - .01f ) - layout_.width, (int)std::ceil( getHeight() / scale - .01f ) - layout_.height );
            board_->setTransform( juce::AffineTransform::scale( scale ) );
            return;
        }
        const auto x = ( getWidth() - layout_.width * scale ) / 2.0f, y = ( getHeight() - layout_.height * scale ) / 2.0f;
        board_->setTransform( juce::AffineTransform::scale( scale ).translated( x, y ) );
    }

    void Panel::timerCallback()
    {
        const auto disconnected=!engine_.connected();
        board_->setEnabled(!disconnected && !engine_.soundBusy());
        recovery_.update();
    }
}
