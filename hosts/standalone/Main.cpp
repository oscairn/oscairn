// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oscairn standalone: the demo instrument in a desktop window, with a choice of panel
// layouts (View menu, or --layout NAME|FILE.json). Every MIDI input plays it.
//
//   oscairn-standalone [--profile NAME|path.json] [--controller path.json]... [--layout NAME|path.json] [--plugins DIR]
//                      [--in-process | --connect SOCKET] [--play] [--input-mode automatic|mouse|touch]
//                      [--app performance|library|setlist|groovebox|recordings|settings|plugins]
//                      [--snapshot out.png [--size WxH] [--play SECONDS] [--view TAB]]
//   oscairn-standalone --version
//
// --profile names the device it runs on (RFC 0024): a built-in profile (qemu-aarch64, kick, ignite,
// modx-m) or a file; without it, the computer's own. A panel profile fills the display with no
// title bar, full screen under any window manager, everything else inside it; shows its layout and asks the audio device for its block size; --layout and the
// audio options override it for a run. --play starts the sequencer at launch. --input-mode overrides the saved preference for this run. --snapshot
// renders the window off screen (no window or splash is shown) to a PNG and quits: at the layout's design size, or as in a window of
// --size; with --play SECONDS, the sequencer plays for that long first; with --view, the
// desktop shows that tab (Part, Mixer, Routing or Controls) or Play. External MIDI controllers
// with a profile (built in: smk37-elite, modx-m-usb; or --controller FILE, as many as wanted) join
// the device's controls when their MIDI ports are connected (RFC 0024); not with --snapshot or --audio null.

#include "Controllers.h"
#include "DesktopShell.h"
#include "DeviceScreen.h"
#include "Engine.h"
#include "Layout.h"
#include "oscairn/item_types.h"
#include "LibraryPane.h"
#include "Launcher.h"
#include "KeyboardWindow.h"
#include "Panel.h"
#include "Profile.h"
#include "PluginBrowser.h"
#include "PadEditor.h"
#include "PartsView.h"
#include "PerformanceColors.h"
#include "PlayView.h"
#include "Recording.h"
#include "AppProcesses.h"
#include "Splash.h"
#include "TextKeyboard.h"
#include "Executable.h"
#include "PaintCounter.h"

#include <BinaryData.h>
#include <oscairn/plugin_host.h>
#include <juce_gui_extra/juce_gui_extra.h>

#include <cstdio>
#include <memory>

namespace oscairn::standalone
{
    namespace
    {
        struct NamedLayout
        {
            juce::String name; // the file name without .json
            juce::String json;
        };

        std::vector<NamedLayout> builtInLayouts()
        {
            std::vector<NamedLayout> layouts;
            for( int i = 0; i < BinaryData::namedResourceListSize; ++i )
            {
                int         size = 0;
                const auto* data = BinaryData::getNamedResource( BinaryData::namedResourceList[i], size );
                const auto  file = juce::String( BinaryData::getNamedResourceOriginalFilename( BinaryData::namedResourceList[i] ) );
                if( data && file.endsWith( ".json" ) )
                    layouts.push_back( { file.upToLastOccurrenceOf( ".json", false, false ), juce::String::fromUTF8( data, size ) } );
            }
            // A fixed order for the menu.
            const juce::StringArray order{ "desktop", "kick", "kick-screen", "ignite", "keys49" };
            std::sort( layouts.begin(), layouts.end(), [&]( const NamedLayout& a, const NamedLayout& b ) {
                return order.indexOf( a.name ) < order.indexOf( b.name );
            } );
            return layouts;
        }

        juce::String argument( const juce::StringArray& args, const juce::String& name )
        {
            const auto i = args.indexOf( name );
            return i >= 0 && i + 1 < args.size() ? args[i + 1] : juce::String();
        }
        // --profile as apps that are processes read it (OSCAIRN_PROFILE): a built-in's id, or the file's full path.
        std::string profileSource( const juce::StringArray& args )
        {
            const auto given = argument( args, "--profile" ).unquoted();
            const auto file = juce::File::getCurrentWorkingDirectory().getChildFile( given );
            return ( given.isNotEmpty() && file.existsAsFile() ? file.getFullPathName() : given ).toStdString();
        }

        // --snapshot: no window goes on the desktop; each renders off screen (setOffscreen).
        bool offscreen = false;

        // The apps that run as processes: Apps/ beside this program, else share/oscairn/apps.
        juce::File appsFolder()
        {
            const auto binary = executableFile().getParentDirectory();
            return binary.getChildFile( "Apps" ).isDirectory() ? binary.getChildFile( "Apps" ) : binary.getSiblingFile( "share" ).getChildFile( "oscairn/apps" );
        }
    }

    /// An app in a window of its own, apart from the sound editor; closing it hides it.
    class AppWindow final : public juce::DocumentWindow {
    public:
        AppWindow(const juce::String& name, juce::Component* content, int width, int height) : DocumentWindow("Oscairn - " + name, ink(Ink::Background), DocumentWindow::allButtons, !offscreen)
        {
            if(offscreen) setOffscreen(*this);
            setUsingNativeTitleBar(true); setContentOwned(content, false); setResizable(true, false); centreWithSize(width, height);
        }
        std::function<void()> onClose;
        void closeButtonPressed() override { if(onClose) onClose(); else setVisible(false); }
    };

    class LibraryWindow final : public juce::DocumentWindow {
    public:
        LibraryWindow(Engine& engine,Theme& theme,const juce::File& plugins):DocumentWindow("Oscairn - Plugins and appearance",ink(Ink::Background),DocumentWindow::allButtons,!offscreen) {
            if(offscreen)setOffscreen(*this);
            setUsingNativeTitleBar(true);setContentOwned(new PluginBrowser(engine,theme,plugins),true);
            setResizable(true,false);setResizeLimits(900,620,1800,1200);centreWithSize(1060,760);setVisible(true);
        }
        void closeButtonPressed() override {setVisible(false);}
    };

    class Window : public juce::DocumentWindow, public juce::MenuBarModel
    {
    public:
        Window( Engine& engine, std::vector<NamedLayout> layouts, const Layout& first, Profile profile,Theme& theme,const juce::File& plugins, InputMode mode, const std::string& profileArgument )
            : DocumentWindow( "Oscairn", juce::Colours::black, DocumentWindow::allButtons, !offscreen )
            , engine_( engine )
            , layouts_( std::move( layouts ) )
            , profile_( std::move( profile ) )
            , fullScreen_( profile_.screen() && profile_.screen()->surface != "desktop" ),theme_(theme),plugins_(plugins),inputMode_(mode)
        {
            const bool fullScreen = fullScreen_; // a device's screen: no title bar, no menu bar
            if( offscreen ) setOffscreen( *this );
            setUsingNativeTitleBar( !fullScreen );
            if( fullScreen )
            {
                setTitleBarHeight( 0 );
                // A dense screen scales everything, dialogs and menus too, so a touch target keeps
                // its physical size, on whatever display (RFC 0019).
                setDeviceScale( profile_.screen()->scale() );
                capFrameRate(); // after the scale, which reads the displays again; the window takes it when it fills the screen
                // Nothing else opens as a window of its own: dialogs, alerts and menus open inside this one.
                setDeviceScreen( this, Profile::Screen::touchTarget );
                setDense( *this ); // and dense by design: touch-size controls, the least space between them
            }
#if JUCE_MAC
            juce::MenuBarModel::setMacMainMenu( this );
#else
            if( !fullScreen ) // a device's screen has no menu bar
                setMenuBar( this );
#endif
            if( profile_.screen() && profile_.screen()->textOnScreen() ) setTextEntryOnScreen( true ); // no typing keyboard: text fields bring up their own
            engine_.setControllers( controlGroups( profile_ ), {} );
            engine_.openBrowser=[this]{showLibrary();};
            engine_.openPads=[this]{showPads();};
            engine_.openParts=[this]{showParts();};
            engine_.goHome=[this]{showHome();};
            engine_.goBack=[this]{showHome();}; // shortcut: every app's top screen is one level down; deeper levels go back inside their apps
            // The on-screen keyboards, where the profile offers them: windows on the desktop, a sheet over a device's screen.
            if( profile_.onScreenKeyboard )
            {
                keyboards_ = std::make_unique<Keyboards>( engine_, theme_.configuration(), fullScreen_ ? this : nullptr );
                keyboards_->channel = [this] { auto* s = shell(); return s ? s->play().selectedChannel() : -1; };
                keyboards_->mode = [this] { return panel() || inputMode_ == InputMode::Touch ? 2 : 1; };
                engine_.openKeyboard = [this]( const std::string& input ) { keyboards_->open( input ); };
                engine_.onNotesOff = [this] { keyboards_->releaseAll(); };
            }

            // What the built-in apps offer each other (RFC 0022): the Library picks, an item of
            // every type it indexes (RFC 0023: oscairn.pick.<kind>); Play hears the setlist's
            // entry playing. Apps that are processes (the Setlist) offer theirs from their
            // manifests, found in Apps/ beside this program.
            // The engine's types, those packages installed there too (step 3), as it knows them at the start; Oscairn's own when it does not answer.
            std::vector<std::string> kinds;
            for(const auto& type:engine_.libraryTypes())if(type.storage=="managed" || type.storage=="in-place")kinds.push_back(type.kind);
            if(kinds.empty())for(const auto& type:itemTypes())if(type.indexed())kinds.push_back(type.kind());
            for(const auto& kind:kinds)
                actions_.offer("oscairn.pick."+kind,"Library",[this,kind](const std::string&,Actions::Answer answer){showPicker(std::move(answer),kind);});
            actions_.offer(sendSetlistEntry,"Performance",[this](const std::string& id,Actions::Answer answer){setlistEntry_=id;answer("shown");});
            const auto binary=executableFile().getParentDirectory();
            // Apps run as processes only against an engine on a socket (not --in-process).
            appProcesses_=std::make_unique<AppProcesses>(engine_,actions_,engine_.canPrepareSounds()?findApps(appsFolder()):std::vector<AppManifest>{},std::vector<std::string>{"OSCAIRN_THEMES="+binary.getChildFile("themes").getFullPathName().toStdString(),"OSCAIRN_PROFILE="+profileArgument,"OSCAIRN_SCREEN_THEME="+(profile_.screen()?profile_.screen()->theme:std::string()),"OSCAIRN_TEXT_ENTRY="+(profile_.screen()&&profile_.screen()->textOnScreen()?std::string("onScreen"):std::string("keyboard"))});
            appNotice_=std::make_unique<AppNotice>(*appProcesses_);
            menuItemsChanged(); // the Apps menu again, now that it knows the Setlist app
            Component::addChildComponent(*appNotice_);
            appProcesses_->onNotice=[this](const AppProcesses::Notice& n){
                setDesktopMode(*appNotice_,panel() || inputMode_==InputMode::Touch?2:1);appNotice_->show(n);resized();
                if(n.text.isEmpty() || n.choice!=AppProcesses::Notice::Choice::None)return;
                // A notice of what happened goes by itself; one asking for a choice stays.
                juce::Timer::callAfterDelay(8000,[safe=juce::Component::SafePointer<Window>(this),text=n.text]{if(safe && safe->appNotice_->text()==text)safe->appNotice_->show({});});
            };
            // Why the last Performance did not open at start (RFC 0028), said as an app's notice is.
            engine_.onReopened=[this](const std::string& why){if(!why.empty())appProcesses_->onNotice({{},juce::String::fromUTF8(why.c_str())});};
            show( first );
            setVisible( true );
        }

        ~Window() override
        {
            engine_.onReopened={};
            engine_.openBrowser={};
            engine_.openPads={};
            engine_.openParts={};
            engine_.goHome={};
            engine_.goBack={};
            engine_.openKeyboard={};
            engine_.onNotesOff={};
#if JUCE_MAC
            juce::MenuBarModel::setMacMainMenu( nullptr );
#else
            setMenuBar( nullptr );
#endif
        }

        void show( const Layout& layout )
        {
            pads_.reset();parts_.reset();
            // The last layout's Home and running apps, timers and all, once its panel lets go of them.
            if( auto* p = panel() ) p->overlay( nullptr );
            for( const auto& r : running_ ) engine_.appClosed( appId( r.name ) );
            switcher_.reset(); running_.clear(); launcher_.reset();
            layout_ = layout;
            // Home first (RFC 0030): the window's content on the desktop, over a panel's screen.
            if( layout.surface != Layout::Surface::Desktop )
            {
                auto* p = new Panel( engine_, layout );
                // A device's screen: the layout reflows to the screen's size in design units.
                if( const auto* s = profile_.screen(); fullScreen_ && s )
                    p->setDesignSize( juce::roundToInt( s->width / s->scale() ), juce::roundToInt( s->height / s->scale() ) );
                setContentOwned( p, false );
            }
            showHome();
            setName( "Oscairn - " + juce::String( layout.name ) );

            // Start at the layout's design size (one design unit per pixel) if it fits,
            // otherwise as large as fits.
            const auto area  = juce::Desktop::getInstance().getDisplays().getPrimaryDisplay()->userBounds.reduced( 40 );
            const auto scale = juce::jmin( 1.0f, area.getWidth() / (float)layout.width, area.getHeight() / (float)layout.height );
            setResizable( layout.resize != Layout::Resize::Fixed, false );
            if( auto* c = getConstrainer() )
                c->setFixedAspectRatio( layout.resize == Layout::Resize::Scale ? layout.width / (double)layout.height : 0.0 );
            setResizeLimits( layout.width / 4, layout.height / 4, layout.width * 3, layout.height * 3 );
            if( fullScreen_ )
            {
                // The whole display, whatever its shape: the panel scales or stretches to fit.
                setResizeLimits( 1, 1, 1 << 15, 1 << 15 );
                if( auto* c = getConstrainer() )
                    c->setFixedAspectRatio( 0.0 );
                fillScreen( *this ); // full screen and undecorated, under a window manager too
                return;
            }
            centreWithSize( juce::roundToInt( layout.width * scale ), juce::roundToInt( layout.height * scale ) );
        }

        void closeButtonPressed() override { juce::JUCEApplication::getInstance()->systemRequestedQuit(); }
        // A device's screen: no border round it to draw or drag (the window's own, with no native title bar).
        juce::BorderSize<int> getBorderThickness() const override { return fullScreen_ ? juce::BorderSize<int>() : DocumentWindow::getBorderThickness(); }

        void resized() override
        {
            DocumentWindow::resized();
            if( pads_ && getContentComponent() ) pads_->setBounds( getContentComponent()->getBounds() );
            if( notice_ ) notice_->setBounds( getLocalBounds() );
            if( appNotice_ ) appNotice_->setBounds( getLocalBounds().reduced( 8 ).removeFromTop( desktopMode( *appNotice_ ) == 2 ? 56 : 40 ) );
            if( parts_ && parts_->getParentComponent() == this && getContentComponent() ) parts_->setBounds( getContentComponent()->getBounds() ); // a panel places its own
        }

        // Cmd-Z undoes and Cmd-Shift-Z redoes (Ctrl elsewhere), from any view: keys the views do
        // not use reach the window. Not Ctrl-Y: the on-screen keyboard plays Y as a note.
        bool keyPressed( const juce::KeyPress& key ) override
        {
            const auto mods = key.getModifiers();
            const int code = key.getKeyCode();
            if( !mods.isCommandDown() || mods.isAltDown() ) return false;
            if( (code == 'Z' || code == 'z') && !mods.isShiftDown() ) { engine_.edit( []( Control& c ) { c.undo(); } ); return true; }
            if( (code == 'Z' || code == 'z') && mods.isShiftDown() ) { engine_.edit( []( Control& c ) { c.redo(); } ); return true; }
            return false;
        }

        const Layout& layout() const { return layout_; }
        // What --snapshot renders: an app's own window when --app opened one, else this window.
        juce::Component* snapshotView() const { return snapshotView_ ? snapshotView_ : getContentComponent(); }

        // Home (RFC 0030): the launcher, as the window's content on the desktop, over the screen
        // on a panel. What it replaces goes after it is in place: a panel points at its overlay.
        void showHome()
        {
            if( auto* p = panel() )
            {
                // The running apps keep their state behind it (RFC 0030).
                if( !launcher_ ) { launcher_ = std::make_unique<Launcher>( engine_, launcherApps(), barShell() ); setDesktopMode( *launcher_, 2 ); }
                // A compact screen's own lists are its Home, until a device has its system screens.
                p->overlay( p->layout().surface == Layout::Surface::Compact ? nullptr : launcher_.get() );
                switcher_.reset();
                return;
            }
            auto* home = new Launcher( engine_, launcherApps(), barShell() );
            setDesktopMode( *home, inputMode_ == InputMode::Touch ? 2 : 1 );
            if( shell() ) engine_.appClosed( "oscairn.performance" ); // Home replaces it
            setContentOwned( home, false );
        }
        // An app by name, as its tile opens it (--app): performance, library, setlist, groovebox, recordings or settings; or plugins.
        void openApp( const juce::String& name )
        {
            if( name == "performance" ) openPerformance();
            else if( name == "library" ) showLibraryApp();
            else if( name == "setlist" ) showSetlist();
            else if( name == "groovebox" ) openGroovebox();
            else if( name == "recordings" ) showRecordings();
            else if( name == "settings" && settingsApp() ) showSettings();
            else if( name == "plugins" ) showLibrary(); // as the Plugins button opens it
            else throw std::invalid_argument( "--app is performance, library, setlist, groovebox, recordings, settings or plugins" );
            // On the desktop those four open in windows of their own: --snapshot shows that window.
            if( !panel() )
                snapshotView_ = name == "library" && libraryApp_ ? libraryApp_->getContentComponent()
                              : name == "plugins" && library_ ? library_->getContentComponent()
                              : name == "groovebox" && groovebox_ ? groovebox_->getContentComponent()
                              : name == "recordings" && recordings_ ? recordings_->getContentComponent() : nullptr;
        }
        // The Performance app: playing the open Performance (Play, first) and editing it.
        void openPerformance()
        {
            if( !admitted( "Performance", [this] { openPerformance(); } ) ) return;
            if( panel() ) { showPerformanceApp( false ); return; }
            auto* shell = new DesktopShell( engine_, scanner(), inputMode_ );
            followSetlist( shell->play() );
            if( keyboards_ ) keyboards_->offerHint( shell->play() );
            shell->onHome = [this] { showHome(); };
            shell->onSave = [this] { saveToLibrary( false ); };
            setContentOwned( shell, false );
        }
        DesktopShell* shell() const { return dynamic_cast<DesktopShell*>( getContentComponent() ); }

        juce::StringArray getMenuBarNames() override { return { "Performance", "Edit", "View", "Plugins", "Appearance", "Apps", "Part" }; }

        juce::PopupMenu getMenuForIndex( int index, const juce::String& ) override
        {
            juce::PopupMenu menu;
            if(index==0){const bool free=engine_.connected() && !engine_.soundBusy();menu.addItem(5002,"Save",free);menu.addItem(5003,"Save As New...",free);
                menu.addItem(5004,"Import Performance...",free && actions_.offered(pickPerformance));
                menu.addItem(5005,"Export Performance...",free);
                menu.addItem(5006,"Install Library...",free);
                // The color lists and setlists show it in; saved with the Performance.
                if(const auto info=engine_.performanceInfo()){juce::PopupMenu colors;addColors(colors,info->color,5100,"None");menu.addSubMenu("Color",colors,free);}
                menu.addSeparator();menu.addItem(5000,"Save to Folder...",free);menu.addItem(5001,"Open Folder...",free);return menu;}
            if(index==1){const auto h=engine_.history();
                menu.addItem(6000,h.undo.empty()?juce::String("Undo"):"Undo "+juce::String::fromUTF8(h.undo.c_str()),!h.undo.empty());
                menu.addItem(6001,h.redo.empty()?juce::String("Redo"):"Redo "+juce::String::fromUTF8(h.redo.c_str()),!h.redo.empty());return menu;}
            if(index==3){menu.addItem(2000,"Browse plugins and edit parameters...");return menu;}
            // Apps that play the sounds, each in its own window, apart from the sound editor.
            // The selected part's Sound (RFC 0021): from the library, or saved to it.
            if(index==6){const bool part=shell() && shell()->selectedPart() && engine_.connected() && !engine_.soundBusy();
                // Update when the library has a newer revision of the Sound the part came from.
                PartSource from;if(part)for(const auto& p:engine_.graph().parts)if(p.id==shell()->selectedPart())from=p.from;
                bool newer=false;
                if(part && !from.sound.empty())if(const auto item=engine_.libraryItem(from.sound))newer=!item->id.empty() && item->revision!=from.revision;
                menu.addItem(8100,"Load Sound into Part...",part && actions_.offered(pickSound));
                menu.addItem(8104,"Update from Library",newer);
                menu.addSeparator();
                menu.addItem(8101,from.sound.empty()?juce::String("Save Sound..."):"Save Sound",part);
                menu.addItem(8102,"Save Sound As New...",part);
                return menu;}
            // The apps this device has (its profile's).
            if(index==5){
                menu.addItem(8003,"Home");menu.addSeparator();
                if(profile_.offers("oscairn.performance"))menu.addItem(8004,"Performance");
                if(profile_.offers("oscairn.groovebox"))menu.addItem(8000,"Groovebox");
                if(profile_.offers("oscairn.library"))menu.addItem(8001,"Library");
                if(profile_.offers("oscairn.setlist") && appProcesses_ && appProcesses_->find("oscairn.setlist"))menu.addItem(8002,"Setlist"); // the menu is first built before the app processes
                if(profile_.offers("oscairn.recordings"))menu.addItem(8005,"Recordings");
                menu.addSeparator();menu.addItem(8006,"Settings...",settingsApp()); // the Settings app; nothing to open with --in-process
                return menu;}
            if(index==4){themeItems_=theme_.installed();for(size_t i=0;i<themeItems_.size();++i)menu.addItem(3000+int(i),themeItems_[i].second,true,themeItems_[i].first==themeData().id);menu.addSeparator();menu.addItem(4000,"Manage themes...");return menu;}
            for( size_t i = 0; i < layouts_.size(); ++i )
            {
                const auto& l      = layouts_[i];
                const auto  ticked = juce::String( layout_.name ).equalsIgnoreCase( title( l ) );
                menu.addItem( (int)i + 1, title( l ), true, ticked );
            }
            menu.addSeparator();
            menu.addItem( 1000, "Open Layout File..." );
            // How the desktop is used: control sizes and spacing (RFC 0019).
            menu.addSeparator();
            juce::PopupMenu input;
            for( auto mode : { InputMode::Automatic, InputMode::Mouse, InputMode::Touch } )
                input.addItem( 7000 + int( mode ), inputModeName( mode ), true, mode == inputMode_ );
            menu.addSubMenu( "Input", input, layout_.surface == Layout::Surface::Desktop );
            if( keyboards_ ) menu.addItem( 7100, "On-screen Keyboard" );
            return menu;
        }

        void menuItemSelected( int id, int ) override
        {
            if(id==5000 || id==5001) {
                const bool save=id==5000;
                const auto initial=save?juce::File::getSpecialLocation(juce::File::userDocumentsDirectory).getChildFile("Untitled.oscairn"):juce::File();
                chooser_=std::make_unique<juce::FileChooser>(save?"Save Performance to a new folder":"Open Performance folder",initial,save?"*.oscairn":"*");
                chooser_->launchAsync((save?juce::FileBrowserComponent::saveMode:juce::FileBrowserComponent::openMode)|juce::FileBrowserComponent::canSelectDirectories,
                    [safe=juce::Component::SafePointer<Window>(this),save](const juce::FileChooser& fc) {
                        if(!safe || fc.getResult()==juce::File())return;
                        auto go=[safe,save,folder=fc.getResult()]{
                            if(!safe)return;
                            SoundAction action;action.kind=save?SoundAction::SavePerformance:SoundAction::LoadPerformance;
                            action.directory=folder.getFullPathName().toStdString();safe->engine_.soundAction(std::move(action));};
                        if(save)go();else askToSave(safe->engine_,safe.getComponent(),"opening "+fc.getResult().getFileNameWithoutExtension(),go); // it replaces the open one
                    });return;
            }
            if(id>=7000 && id<=7002){
                inputMode_=InputMode(id-7000);writeInputMode(theme_.configuration(),inputMode_);if(auto* s=shell())s->setInputMode(inputMode_);
                // Open app windows take the new sizes now, not when next resized.
                for(auto* w:{groovebox_.get(),libraryApp_.get(),picker_.get()})
                    if(auto* content=w?w->getContentComponent():nullptr){setDesktopMode(*content,inputMode_==InputMode::Touch?2:1);content->resized();}
                return;}
            if(id==7100 && keyboards_){keyboards_->open({});return;}
            if(id==8000){openGroovebox();return;}
            if(id==8001){showLibraryApp();return;}
            if(id==8002){showSetlist();return;}
            if(id==8003){showHome();return;}
            if(id==8004){openPerformance();return;}
            if(id==8005){showRecordings();return;}
            if(id==8006){if(settingsApp())showSettings();return;} // nothing to open with --in-process: the item is disabled
            if(id==5002 || id==5003){saveToLibrary(id==5003);return;}
            if(id==5004){importPerformance();return;}
            if(id==5005){exportPerformance();return;}
            if(id==5006){showApp(libraryApp_,"Library",[this]{return libraryPane(LibraryPane::Mode::Play);},900,700);
                if(auto* pane=dynamic_cast<LibraryPane*>(libraryApp_->getContentComponent()))pane->install();return;}
            if(id==8100){loadSound();return;}
            if(id==8101 || id==8102){saveSound(id==8102);return;}
            if(id==8104){updateSound();return;}
            if(std::string color;colorChosen(id,5100,color)){engine_.edit([color](Control& c){auto info=c.performanceInfo();info.color=color;c.setPerformanceInfo(info);});return;}
            if(id==6000){engine_.edit([](Control& c){c.undo();});return;}
            if(id==6001){engine_.edit([](Control& c){c.redo();});return;}
            if(id==2000 || id==4000){showLibrary();return;}
            if(id>=3000 && size_t(id-3000)<themeItems_.size()){try{theme_.apply(themeItems_[size_t(id-3000)].first);}catch(const std::exception& e){juce::AlertWindow::showMessageBoxAsync(juce::MessageBoxIconType::WarningIcon,"Theme not applied",e.what());}return;}
            if( id == 1000 )
            {
                chooser_ = std::make_unique<juce::FileChooser>( "Open a panel layout", juce::File(), "*.json" );
                chooser_->launchAsync( juce::FileBrowserComponent::openMode | juce::FileBrowserComponent::canSelectFiles,
                                       [this]( const juce::FileChooser& fc ) {
                                           if( fc.getResult() != juce::File() )
                                               tryShow( fc.getResult().loadFileAsString() );
                                       } );
            }
            else if( id >= 1 && id <= (int)layouts_.size() )
                tryShow( layouts_[(size_t)id - 1].json );
        }

    private:
        // Oscairn's own apps by name, as RFC 0025's memory reservations name them.
        static std::string appId( const juce::String& name ) { return "oscairn." + name.toLowerCase().toStdString(); }
        // Asked of the engine before an app opens (RFC 0025), with its reservation (an app that is
        // a process: its manifest's, or what it was measured using), else its placeholder; refused, a notice over the window says
        // why, and may let the next song go and try again.
        // shortcut: an app's window closed on the desktop hides it with its state, so its reservation stays; the desktop has no memory classes yet.
        bool admitted( const juce::String& name, std::function<void()> retry )
        {
            const auto id = appId( name );
            const auto reserve = appProcesses_->find( id ) ? appProcesses_->reservation( id ) : uint64_t( Profile::appMemory( id ) ) << 20;
            return askToLaunch( engine_, { id, reserve }, name, std::move( retry ),
                [this]( std::unique_ptr<LaunchNotice> notice ) {
                    notice_ = std::move( notice );
                    if( !notice_ ) return;
                    setDesktopMode( *notice_, panel() || inputMode_ == InputMode::Touch ? 2 : 1 );
                    addAndMakeVisible( *notice_ ); notice_->setBounds( getLocalBounds() );
                } );
        }
        void showGroovebox(){showApp(groovebox_,"Groovebox",[this]{return new SequencerPane(engine_);},1100,760);}
        // The Recordings app (RFC 0020): the takes, kept running with its state like the others.
        void showRecordings()
        {
            if( !admitted( "Recordings", [this] { showRecordings(); } ) ) return;
            if( panel() )
            {
                if( bringForward( "Recordings" ) ) return;
                auto pane = std::make_unique<RecordingsPane>( engine_, theme_.configuration(), false ); auto& app = *pane; openOnScreen( "Recordings", "audio-lines", app, std::move( pane ) ); return;
            }
            showApp( recordings_, "Recordings", [this] { return new RecordingsPane( engine_, theme_.configuration() ); }, 760, 600 );
        }
        Panel* panel() const { return dynamic_cast<Panel*>( getContentComponent() ); }
        bool layoutBinds( const std::string& bind ) const
        {
            return std::any_of( layout_.controls.begin(), layout_.controls.end(), [&]( const Layout::Control& c ) { return c.bind == bind; } );
        }
        // What the top bar's buttons do (RFC 0030): each shown only where the layout has no button
        // for it; the running apps on a panel; Back goes Home for now (every app's top screen is
        // one level down; going back inside an app is the app's own).
        TopBar::Shell barShell()
        {
            TopBar::Shell shell;
            if( !layoutBinds( "home" ) ) shell.home = [this] { showHome(); };
            if( !layoutBinds( "back" ) ) shell.back = [this] { showHome(); };
            if( panel() ) shell.apps = [this] { showSwitcher(); };
            if( !layoutBinds( "panic" ) ) shell.notesOff = [this] { engine_.allNotesOff(); };
            shell.save = [this]( bool asNew ) { saveToLibrary( asNew ); };
            shell.record = true;
            return shell;
        }
        // A running app on a panel's screen brought forward, if it is running.
        bool bringForward( const juce::String& name )
        {
            for( auto& r : running_ )
                if( r.name == name ) { panel()->overlay( r.frame.get() ); switcher_.reset(); return true; }
            return false;
        }
        // An app on a panel's screen, in its frame under the top bar, kept running with its state.
        void openOnScreen( const juce::String& name, const char* icon, juce::Component& app, std::unique_ptr<juce::Component> owned )
        {
            RunningApp r{ name, icon, std::move( owned ), &app, nullptr };
            r.frame = std::make_unique<AppFrame>( engine_, name, app, barShell() );
            setDesktopMode( *r.frame, 2 ); setDesktopMode( app, 2 );
            panel()->overlay( r.frame.get() );
            running_.push_back( std::move( r ) );
            switcher_.reset();
        }
        // The running apps, to switch to one or close it.
        void showSwitcher()
        {
            std::vector<AppSwitcher::Running> list;
            for( const auto& r : running_ ) list.push_back( { r.name, r.icon } );
            auto next = std::make_unique<AppSwitcher>( engine_, list,
                [this]( int i ) { if( size_t( i ) < running_.size() ) bringForward( running_[size_t( i )].name ); },
                [this]( int i ) {
                    if( size_t( i ) >= running_.size() ) return;
                    engine_.appClosed( appId( running_[size_t( i )].name ) );
                    if( auto* place = dynamic_cast<AppPlace*>( running_[size_t( i )].app ) ) appProcesses_->stop( place->id() ); // a process: it quits
                    running_.erase( running_.begin() + i ); // not shown: the switcher is in front
                    showSwitcher(); },
                barShell() );
            setDesktopMode( *next, 2 );
            panel()->overlay( next.get() );
            switcher_ = std::move( next );
        }
        // The apps Home offers: the profile's.
        std::vector<Launcher::App> launcherApps()
        {
            std::vector<Launcher::App> apps;
            if( profile_.offers( "oscairn.performance" ) )
                apps.push_back( { "Performance", "piano", [this] {
                const auto info = engine_.performanceInfo();
                return info ? juce::String::fromUTF8( info->name.empty() ? "Untitled" : info->name.c_str() ) + ": play or edit it" : juce::String(); },
                [this] { openPerformance(); } } );
            if( profile_.offers( "oscairn.library" ) )
                apps.push_back( { "Library", "library-big", [this] {
                    Control::LibraryQuery q; q.limit = 1; q.kind = "performance";
                    const auto performances = engine_.library( q ); q.kind = "sound"; const auto sounds = engine_.library( q );
                    if( !performances || !sounds ) return juce::String();
                    return libraryLine( installedPluginPackages( engine_ ), performances->total, sounds->total ); },
                    [this] { showLibraryApp(); } } );
            if( profile_.offers( "oscairn.setlist" ) && appProcesses_->find( "oscairn.setlist" ) )
                apps.push_back( { "Setlist", "list-music", [] { return juce::String( "Songs in order, for the stage" ); }, [this] { showSetlist(); } } );
            if( profile_.offers( "oscairn.groovebox" ) )
                apps.push_back( { "Groovebox", "layout-grid", [] { return juce::String( "Patterns and steps" ); }, [this] { openGroovebox(); } } );
            if( profile_.offers( "oscairn.recordings" ) )
                apps.push_back( { "Recordings", "audio-lines", [this] {
                    const auto n = int( engine_.takes().size() );
                    return n ? juce::String( n ) + ( n == 1 ? " take" : " takes" ) : juce::String( "Takes and captures" ); },
                    [this] { showRecordings(); } } );
            if( profile_.offers( "oscairn.settings" ) && settingsApp() ) // the same as the Setlist: a process, so not with --in-process
                apps.push_back( { "Settings", "settings", [this] {
                    const auto device = juce::String( engine_.sessionStatus().audioDevice );
                    if( device == "null" ) return juce::String( "No audio device (--audio null)" ); // the null device's name
                    return device.isNotEmpty() ? device : juce::String( "Audio, controllers, keys" ); },
                    [this] { showSettings(); } } );
            return apps;
        }
        void showLibraryApp()
        {
            if( !admitted( "Library", [this] { showLibraryApp(); } ) ) return;
            if( panel() )
            {
                if( bringForward( "Library" ) ) return;
                std::unique_ptr<juce::Component> pane( libraryPane( LibraryPane::Mode::Play ) ); auto& app = *pane; openOnScreen( "Library", "library-big", app, std::move( pane ) ); return;
            }
            showApp( libraryApp_, "Library", [this] { return libraryPane( LibraryPane::Mode::Play ); }, 900, 700 );
        }
        // The Settings app, its own process (RFC 0022), in a window of its own; only against an engine on a socket.
        bool settingsApp() const { return appProcesses_ && appProcesses_->find( "oscairn.settings" ); }
        void showSettings()
        {
            if( !admitted( "Settings", [this] { showSettings(); } ) ) return;
            if( panel() ) // as the Setlist: its window over a frame under the top bar
            {
                if( bringForward( "Settings" ) ) return;
                auto place = std::make_unique<AppPlace>( *appProcesses_, "oscairn.settings" ); auto& app = *place; openOnScreen( "Settings", "settings", app, std::move( place ) ); return;
            }
            appProcesses_->open( "oscairn.settings", {}, inputMode_ == InputMode::Touch ? 2 : 1 );
        }
        void showSetlist()
        {
            if( !admitted( "Setlist", [this] { showSetlist(); } ) ) return;
            // Its own process (RFC 0022): on a panel, its window over a frame under the top bar.
            if( panel() )
            {
                if( bringForward( "Setlist" ) ) return;
                auto place = std::make_unique<AppPlace>( *appProcesses_, "oscairn.setlist" ); auto& app = *place; openOnScreen( "Setlist", "list-music", app, std::move( place ) ); return;
            }
            appProcesses_->open( "oscairn.setlist", {}, inputMode_ == InputMode::Touch ? 2 : 1 );
        }
        void openGroovebox()
        {
            if( !admitted( "Groovebox", [this] { openGroovebox(); } ) ) return;
            if( panel() ) // Play and Stop on the screen, unless the layout has buttons for them (a device's own come through its engine)
            {
                if( bringForward( "Groovebox" ) ) return;
                const bool buttons = layoutBinds( "play-stop" ) || ( layoutBinds( "play" ) && layoutBinds( "stop" ) );
                auto pane = std::make_unique<SequencerPane>( engine_, !buttons ); auto& app = *pane; openOnScreen( "Groovebox", "layout-grid", app, std::move( pane ) ); return;
            }
            showGroovebox();
        }
        // An app's window, made the first time; it keeps its state while hidden.
        void showApp(std::unique_ptr<AppWindow>& window,const juce::String& name,std::function<juce::Component*()> make,int width,int height)
        {
            if(!window)window=std::make_unique<AppWindow>(name,make(),width,height);
            if(auto* content=window->getContentComponent()){setDesktopMode(*content,inputMode_==InputMode::Touch?2:1);content->resized();}
            window->setVisible(true);window->toFront(true);
        }
        // A Library pane auditioning Sounds in the part the Part view shows.
        LibraryPane* libraryPane(LibraryPane::Mode mode,const std::string& kind="performance")
        {
            auto* pane=new LibraryPane(engine_,actions_,mode,kind);
            pane->auditionPart=[this]{auto* s=shell();return s?s->selectedPart():0u;};
            return pane;
        }
        // The Library's pick, in a window over the asking app; a second pick cancels the first.
        void showPicker(Actions::Answer answer,const std::string& kind)
        {
            if(pickerAnswer_){auto old=std::move(pickerAnswer_);pickerAnswer_={};old({});}
            pickerAnswer_=std::move(answer);
            auto* pane=libraryPane(LibraryPane::Mode::Pick,kind);
            pane->onPicked=[this](const std::string& id){
                auto pending=std::move(pickerAnswer_);pickerAnswer_={};
                // The window goes after this click has finished with it.
                juce::MessageManager::callAsync([safe=juce::Component::SafePointer<Window>(this)]{if(safe && !safe->pickerAnswer_)safe->closePicker();});
                if(pending)pending(id);
            };
            closePicker();
            const auto type=itemType(kind);
            const juce::String title="Choose a "+juce::String::fromUTF8((type?type->name:kind).c_str());
            if(auto* p=panel()){
                // Over the screen, in a frame whose Back cancels; what was over it comes back after.
                auto cancel=barShell();cancel.home=cancel.back=[pane]{pane->onPicked({});};cancel.apps={};
                pickerPane_.reset(pane);pickerFrame_=std::make_unique<AppFrame>(engine_,title,*pane,cancel);
                setDesktopMode(*pickerFrame_,2);setDesktopMode(*pane,2);
                pickerUnder_=p->overlaid();p->overlay(pickerFrame_.get());
                return;
            }
            picker_=std::make_unique<AppWindow>(title,pane,760,620);
            picker_->onClose=[pane]{pane->onPicked({});};
            setDesktopMode(*pane,inputMode_==InputMode::Touch?2:1);
            picker_->setVisible(true);picker_->toFront(true);
        }
        void closePicker()
        {
            picker_.reset();
            if(!pickerFrame_)return;
            if(auto* p=panel())p->overlay(pickerUnder_.getComponent());
            pickerFrame_.reset();pickerPane_.reset();
        }
        // Another Performance's parts and scenes into this one (RFC 0021): the Library picks it,
        // then its parts and scenes are listed, all ticked; the chosen ones come, parts with what
        // they need. Everything ticked is all of it.
        void importPerformance()
        {
            actions_.request(pickPerformance,{},[safe=juce::Component::SafePointer<Window>(this)](const std::string& id){
                if(!safe || id.empty())return;
                const auto item=safe->engine_.libraryItem(id);
                if(!item || item->id.empty())return;
                const auto held=safe->engine_.performanceContents(item->path);
                if(!held)return;
                // No taller than the screen it opens on, less its title bar; the list scrolls past that.
                const auto* display=juce::Desktop::getInstance().getDisplays().getDisplayForRect(safe->getScreenBounds());
                auto* content=new ImportChoices(*held,(display?int(display->userBounds.getHeight()):630)-60);
                juce::DialogWindow::LaunchOptions options;
                options.dialogTitle="Import from "+juce::String::fromUTF8(item->name.c_str());
                options.content.setOwned(content);options.componentToCentreAround=safe.getComponent();options.useNativeTitleBar=true;options.resizable=false;
                options.dialogBackgroundColour=ink(Ink::Background); // JUCE's default is light grey, under the theme's text
                auto* dialog=launchDialog(options);
                content->cancel.onClick=[dialog]{dialog->exitModalState(0);};
                content->import.onClick=[safe,dialog,content,path=item->path]{
                    auto parts=ImportChoices::ticked(content->parts,content->partIds),scenes=ImportChoices::ticked(content->scenes,content->sceneIds);
                    const bool all=parts.size()==content->partIds.size() && scenes.size()==content->sceneIds.size();
                    // A partial import takes a part and, when there are any, a scene.
                    if(!all && (parts.empty() || (scenes.empty() && !content->sceneIds.empty()))){
                        juce::AlertWindow::showMessageBoxAsync(juce::MessageBoxIconType::InfoIcon,"Import","Choose at least one part, and one scene: the scene sets how the parts come.");return;}
                    dialog->exitModalState(0);
                    if(!safe)return;
                    if(all){parts.clear();scenes.clear();} // all of it, loose plugins included
                    SoundAction import;import.kind=SoundAction::ImportPerformance;import.directory=path;import.parts=parts;import.scenes=scenes;safe->engine_.soundAction(std::move(import));
                };
            });
        }
        // The open Performance as one archive file to take to another device (RFC 0021): its
        // samples and its unedited parts' Sounds always go; edited parts' Sounds when asked.
        void exportPerformance()
        {
            const auto info=engine_.performanceInfo();
            const auto name=info && !info->name.empty()?juce::String::fromUTF8(info->name.c_str()):juce::String("Performance");
            chooser_=std::make_unique<juce::FileChooser>("Export the Performance",juce::File::getSpecialLocation(juce::File::userDocumentsDirectory).getChildFile(name+".olib"),"*.olib");
            chooser_->launchAsync(juce::FileBrowserComponent::saveMode|juce::FileBrowserComponent::canSelectFiles|juce::FileBrowserComponent::warnAboutOverwriting,
                [safe=juce::Component::SafePointer<Window>(this)](const juce::FileChooser& fc) {
                    if(!safe || fc.getResult()==juce::File())return;
                    auto file=fc.getResult();if(!file.hasFileExtension("olib"))file=file.withFileExtension("olib");
                    // An existing file is replaced only once the new archive is written (Engine):
                    // Cancel, a refusal or a failed export keeps last week's.
                    showExport(safe->engine_,safe.getComponent(),"Export "+file.getFileNameWithoutExtension(),true,[safe,path=file.getFullPathName().toStdString()](bool editedSounds,const std::string& key){
                        if(!safe)return;
                        SoundAction out;out.kind=SoundAction::ExportPerformance;out.directory=path;out.asNew=editedSounds;out.signKey=key; // signed only when a key was chosen
                        safe->engine_.soundAction(std::move(out));
                    });
                });
        }
        // The library's newer revision of the Sound the selected part came from, in its place.
        void updateSound()
        {
            const auto part=shell()?shell()->selectedPart():0;
            for(const auto& p:engine_.graph().parts)if(p.id==part && !p.from.sound.empty()){
                SoundAction load;load.kind=SoundAction::LoadSound;load.part=part;load.libraryId=p.from.sound;engine_.soundAction(std::move(load));}
        }
        // The Library picks a Sound; it replaces the selected part's chain, keeping the part's seat.
        void loadSound()
        {
            const auto part=shell()?shell()->selectedPart():0;
            if(!part)return;
            actions_.request(pickSound,{},[safe=juce::Component::SafePointer<Window>(this),part](const std::string& id){
                if(!safe || id.empty())return;
                SoundAction load;load.kind=SoundAction::LoadSound;load.part=part;load.libraryId=id;safe->engine_.soundAction(std::move(load));
            });
        }
        // Save writes the next revision of the Sound the part came from, asking for a name only
        // when it came from none; Save As New always asks, and makes another Sound.
        void saveSound(bool asNew)
        {
            const auto part=shell()?shell()->selectedPart():0;
            if(!part)return;
            std::string name;PartSource from;for(const auto& p:engine_.graph().parts)if(p.id==part){name=p.name;from=p.from;}
            if(!asNew && !from.sound.empty()){SoundAction save;save.kind=SoundAction::SaveSound;save.part=part;engine_.soundAction(std::move(save));return;}
            auto* w=new juce::AlertWindow(asNew?"Save Sound As New":"Save Sound","The Sound's name:",juce::MessageBoxIconType::NoIcon,this);
            w->addTextEditor("name",juce::String::fromUTF8(name.c_str()),"Name");
            w->addButton("Save",1,juce::KeyPress(juce::KeyPress::returnKey));w->addButton("Cancel",0,juce::KeyPress(juce::KeyPress::escapeKey));
            keepInScreen(*w);
            w->enterModalState(true,juce::ModalCallbackFunction::create([safe=juce::Component::SafePointer<Window>(this),w,part,asNew](int result){
                if(!safe || result!=1)return;
                SoundAction save;save.kind=SoundAction::SaveSound;save.part=part;save.asNew=asNew;save.choice.name=w->getTextEditorContents("name").trim().toStdString();
                safe->engine_.soundAction(std::move(save));
            }),true);
        }
        void saveToLibrary(bool asNew){savePerformance(engine_,this,asNew);}
        // Plugins and appearance: on a panel's screen as its apps are (the Plugins button), else a window.
        void showLibrary()
        {
            if(panel())
            {
                if(bringForward("Plugins"))return;
                auto browser=std::make_unique<PluginBrowser>(engine_,theme_,plugins_);auto& app=*browser;openOnScreen("Plugins","layers",app,std::move(browser));return;
            }
            if(!library_)library_=std::make_unique<LibraryWindow>(engine_,theme_,plugins_);library_->setVisible(true);library_->toFront(true);
        }
        // The pad editor covers the panel, so it fits a device's single screen too.
        void showPads()
        {
            pads_ = std::make_unique<PadEditor>( engine_ );
            pads_->onDone = [this] { juce::MessageManager::callAsync( [safe = juce::Component::SafePointer<Window>( this )] { if( safe ) safe->pads_.reset(); } ); };
            addAndMakeVisible( *pads_ );
            if( getContentComponent() ) pads_->setBounds( getContentComponent()->getBounds() );
        }
        static std::string scanner() { return executableFile().getSiblingFile( "oscairn-plugin-scan" ).getFullPathName().toStdString(); }
        void showParts()
        {
            if( !panel() && !shell() ) openPerformance(); // from Home: the Performance app has them
            if( auto* s = shell() ) { s->showTab( DesktopShell::Tab::Part ); return; } // the desktop has them in its window
            if( panel() ) { showPerformanceApp( true ); return; } // the Performance app's Edit
            parts_ = std::make_unique<PartsView>( engine_, scanner() );
            parts_->onSave = [this] { saveToLibrary( false ); };
            setDesktopMode( *parts_, 2 ); // a device's screen: the type scale and targets for touch
            parts_->onDone = [this] { juce::MessageManager::callAsync( [safe = juce::Component::SafePointer<Window>( this )] {
                if( !safe ) return;
                safe->showHome(); // done editing: back Home, which lets go of the parts views
            } ); };
            // Over the device's screen, so its knobs and buttons stay in reach, as on the hardware.
            if( auto* panel = dynamic_cast<Panel*>( getContentComponent() ) ) { panel->overlay( parts_.get() ); return; }
            addAndMakeVisible( *parts_ );
            if( getContentComponent() ) parts_->setBounds( getContentComponent()->getBounds() );
        }
        // The Performance app on a panel's screen: Play, or its Edit (the parts views).
        void showPerformanceApp( bool editing )
        {
            if( !admitted( "Performance", [this, editing] { showPerformanceApp( editing ); } ) ) return;
            if( !bringForward( "Performance" ) )
            {
                auto view = std::make_unique<PerformanceApp>( engine_, scanner() );
                followSetlist( view->play() );
                if( keyboards_ ) keyboards_->offerHint( view->play() );
                auto& app = *view; openOnScreen( "Performance", "piano", app, std::move( view ) );
                app.attach( running_.back().frame->bar(), [this] { saveToLibrary( false ); } );
            }
            for( auto& r : running_ ) if( auto* app = r.name == "Performance" ? dynamic_cast<PerformanceApp*>( r.app ) : nullptr ) app->edit( editing );
        }
        // Play's Previous and Next, while the open Performance is the setlist's entry playing,
        // which the Setlist app sends (sendSetlistEntry); they ask it to step (stepSetlist).
        void followSetlist( PlayView& play )
        {
            // While the engine opens the next, which reads no Performance, the last answer holds.
            play.inSetlist = [this, last = false]() mutable {
                if( setlistEntry_.empty() ) return last = false;
                const auto info = engine_.performanceInfo();
                if( !info ) return last;
                return last = info->id == setlistEntry_;
            };
            play.step = [this]( int by ) { actions_.request( stepSetlist, by < 0 ? "-1" : "1", []( const std::string& ) {} ); };
        }
        static juce::String title( const NamedLayout& l )
        {
            try
            {
                return Layout::parse( l.json ).name;
            }
            catch( const std::exception& )
            {
                return l.name;
            }
        }

        void tryShow( const juce::String& json )
        {
            try
            {
                show( Layout::parse( json ) );
            }
            catch( const std::exception& e )
            {
                juce::AlertWindow::showMessageBoxAsync( juce::MessageBoxIconType::WarningIcon, "Layout not loaded", e.what() );
            }
        }

        Engine&                            engine_;
        std::vector<NamedLayout>           layouts_;
        std::unique_ptr<juce::FileChooser> chooser_;
        Profile                            profile_;
        bool                               fullScreen_;
        Theme& theme_;
        juce::File plugins_;
        std::unique_ptr<LibraryWindow> library_;
        Actions actions_;
        std::string setlistEntry_; // the setlist's entry playing, as the Setlist app last said
        std::unique_ptr<AppProcesses> appProcesses_; // before running_: it outlives the places apps draw in
        std::unique_ptr<AppNotice> appNotice_;
        juce::Component* snapshotView_ = nullptr;  // see snapshotView
        std::unique_ptr<Launcher> launcher_;       // Home on a panel
        // A panel's running apps (RFC 0030), each kept with its state: its frame goes before the
        // app it shows (members go last to first).
        struct RunningApp { juce::String name; const char* icon; std::unique_ptr<juce::Component> owned; juce::Component* app; std::unique_ptr<AppFrame> frame; };
        std::vector<RunningApp> running_;
        std::unique_ptr<AppSwitcher> switcher_;
        std::unique_ptr<AppWindow> groovebox_, libraryApp_, picker_, recordings_;
        // The Library's pick over a panel's screen (the pane before the frame that shows it), and what it covers.
        std::unique_ptr<juce::Component> pickerPane_;
        std::unique_ptr<AppFrame> pickerFrame_;
        juce::Component::SafePointer<juce::Component> pickerUnder_;
        std::unique_ptr<LaunchNotice> notice_; // why an app did not open (RFC 0025)
        Actions::Answer pickerAnswer_;
        std::unique_ptr<PadEditor> pads_;
        std::unique_ptr<PartsView> parts_;
        std::unique_ptr<Keyboards> keyboards_; // none where the profile offers no on-screen keyboard
        std::vector<std::pair<juce::String,juce::String>> themeItems_;
        InputMode inputMode_;
        Layout layout_;
    };

    class Application : public juce::JUCEApplication
    {
    public:
        const juce::String getApplicationName() override { return "Oscairn"; }
        const juce::String getApplicationVersion() override { return OSCAIRN_RELEASE; }
        bool               moreThanOneInstanceAllowed() override { return true; }

        void initialise( const juce::String& commandLine ) override
        {
            commandLine_ = commandLine;
            const auto args = juce::StringArray::fromTokens( commandLine, true );
            offscreen = args.contains( "--snapshot" );
#if JUCE_MAC
            if( offscreen || args.contains( "--version" ) || args.contains( "--help" ) ) juce::Process::setDockIconVisible( false ); // never the active app, nor in the Dock
#endif
            if( args.contains( "--version" ) ) { std::printf( "Oscairn %s\n", OSCAIRN_RELEASE ); quit(); return; }
            // Printed, with nothing opened or shown: the arguments at the top of this file.
            if( args.contains( "--help" ) )
            {
                std::printf( "Oscairn [--profile NAME|FILE] [--controller FILE]... [--layout NAME|FILE] [--plugins DIR]\n"
                             "        [--in-process | --connect SOCKET] [--play] [--input-mode automatic|mouse|touch]\n"
                             "        [--app performance|library|setlist|groovebox|recordings|settings|plugins]\n"
                             "        [--snapshot OUT.png [--size WxH] [--play SECONDS] [--view TAB]]\n"
                             "        [the engine's audio options: oscairn-engine --help]\n"
                             "Oscairn --version\n" );
                quit(); return;
            }
            auto       dir  = argument( args, "--plugins" );
            if( dir.isEmpty() )
            {
                const auto binaryDir=executableFile().getParentDirectory();
                const auto bundled=binaryDir.getChildFile("plugins");
                const auto installed=binaryDir.getSiblingFile("lib").getChildFile("clap");
                dir=bundled.isDirectory()?bundled.getFullPathName():installed.isDirectory()?installed.getFullPathName():juce::String(OSCAIRN_PLUGIN_DIR);
            }
            try
            {
                theme_=std::make_unique<Theme>();
                paints_=PaintCounter::fromEnvironment(); // OSCAIRN_UI_PAINT_LOG=1: what the windows draw, a line a second
                host::AudioConfiguration audio;
                bool audioOptions=false;
                const juce::StringArray valued{"--plugins","--connect","--profile","--controller","--layout","--snapshot","--size","--view","--input-mode","--app"};
                std::vector<std::string> controllerFiles;
                for(int i=0;i<args.size();++i)
                {
                    const auto name=args[i].toStdString();
                    if(host::AudioConfiguration::isOption(name) || valued.contains(args[i]))
                    {
                        if(i+1==args.size() || args[i+1].startsWith("--"))throw std::invalid_argument("Missing value for "+name);
                        const auto value=args[++i].unquoted().toStdString();
                        if(host::AudioConfiguration::isOption(name)){audio.setOption(name,value);audioOptions|=name!="--controller";} // a new engine reads the controller files too; one connected to has its own
                        if(name=="--controller")controllerFiles.push_back(value);
                    }
                    else if(name=="--play") {if(i+1<args.size() && !args[i+1].startsWith("--"))++i;}
                    else if(name!="--in-process")throw std::invalid_argument("Unknown argument: "+name);
                }
                // The device this runs on (RFC 0024): its block size for a new engine, unless given.
                const auto profile=loadProfile(argument(args,"--profile").unquoted().toStdString());
                if(profile.screen())theme_->setScreenTheme(profile.screen()->theme); // its screen's theme, unless the user chose one
                const auto controllers=controllerProfiles(controllerFiles); // checked before anything opens
                if(!args.contains("--connect") && !args.contains("--buffer-frames") && profile.audio.block>0)audio.bufferFrames=uint32_t(profile.audio.block);
                // Its audio device and channel map, unless the command line chooses a device or channels.
                if(!args.contains("--connect") && !args.contains("--output-device") && !args.contains("--input-device") && !args.contains("--audio-outputs") &&
                   !args.contains("--audio-inputs") && !args.contains("--device-channels") && !args.contains("--sample-rate"))profile.configureAudio(audio);
                // Its panel, when it has one that is not MIDI: read by a new engine, its map beside the
                // installed program (share/oscairn/panels) unless the profile gives a path.
                if(!args.contains("--connect") && !profile.panel.device.empty())
                {
#if OSCAIRN_ROLAND_FANTOM0
                    const auto exe=executableFile();
                    if(!args.contains("--panel-device"))audio.panelDevice=profile.panel.device;
                    if(!args.contains("--panel-map"))audio.panelMap=juce::File::isAbsolutePath(profile.panel.map)?profile.panel.map
                        :exe.getParentDirectory().getSiblingFile("share/oscairn/panels").getChildFile(profile.panel.map).getFullPathName().toStdString();
#else
                    std::fprintf(stderr,"oscairn: %s's panel is not read: this build has no reader for it (OSCAIRN_ROLAND_FANTOM0)\n",profile.name.c_str());
#endif
                }
                // Its keys, when they are not MIDI: read by a new engine.
                if(!args.contains("--connect") && !profile.keyboard.device.empty())
                {
#if OSCAIRN_ROLAND_FANTOM0
                    if(!args.contains("--keyboard-device"))audio.keyboardDevice=profile.keyboard.device;
                    if(!args.contains("--keyboard-device") && profile.keyboard.localOff)audio.keyboardLocalOff=true;
#else
                    std::fprintf(stderr,"oscairn: %s's keys are not read: this build has no reader for them (OSCAIRN_ROLAND_FANTOM0)\n",profile.name.c_str());
#endif
                }
                // Its input device's keys that are not on the panel (WRITE, MENU and TEMPO): read by a new engine.
                if(!args.contains("--connect") && !profile.inputKeys.name.empty())
                {
#if OSCAIRN_ROLAND_FANTOM0 && defined(__linux__)
                    if(!args.contains("--input-keys"))
                    {
                        audio.inputKeys=profile.inputKeys.name;
                        for(const auto& [function,code]:profile.inputKeys.keys)audio.inputKeyMap[uint16_t(code)]=function;
                    }
#else
                    std::fprintf(stderr,"oscairn: %s's input keys are not read: this build has no reader for them (OSCAIRN_ROLAND_FANTOM0, Linux)\n",profile.name.c_str());
#endif
                }
                audio.validate();
                const auto socket = argument(args, "--connect");
                if(audioOptions && args.contains("--connect"))
                    throw std::invalid_argument("Audio options configure a new engine; omit --connect to use them");
                if(args.contains("--connect") && socket.isEmpty())
                    throw std::invalid_argument("--connect requires a socket path");
                if(args.contains("--in-process") && !socket.isEmpty())
                    throw std::invalid_argument("choose --in-process or --connect");
                std::string helper;
                if(socket.isEmpty() && !args.contains("--in-process"))
                    helper=executableFile().getSiblingFile("oscairn-engine").getFullPathName().toStdString();
                // RFC 0025: the reliability level reaches the engine and its workers, which lock their
                // memory and take their places in the out-of-memory order on Linux; this process when it is the engine.
                setenv( "OSCAIRN_RELIABILITY", profile.reliability.c_str(), 1 );
                // The cores the engine's and workers' audio threads may run on (realtime.h); one set by the session wins.
                if( std::string cores; !profile.compute.audioThreadCores.empty() )
                    { for( auto c : profile.compute.audioThreadCores ) cores += ( cores.empty() ? "" : "," ) + std::to_string( c ); setenv( "OSCAIRN_AUDIO_THREAD_CORES", cores.c_str(), 0 ); }
                // Each plugin worker's allowance per block (realtime.h's workerAllowance); one set by the session wins.
                if( profile.compute.workerAllowance ) setenv( "OSCAIRN_WORKER_BUDGET_US", std::to_string( profile.compute.workerAllowance ).c_str(), 0 );
                // How long an audio thread waiting on a worker spins before it sleeps (realtime.h's spinWindow); likewise.
                if( profile.compute.workerSpin ) setenv( "OSCAIRN_WORKER_SPIN_US", std::to_string( profile.compute.workerSpin ).c_str(), 0 );
                // Each worker waits to its wave's deadline in place of its allowance (RFC 0045 step 6); likewise.
                if( profile.compute.workerDeadline ) setenv( "OSCAIRN_WORKER_DEADLINE", "1", 0 );
                // The plugin hosts the plugins in workers share, and the plugins given workers of their own (RFC 0045); likewise.
                if( profile.compute.pluginHosts ) setenv( "OSCAIRN_PLUGIN_HOSTS", std::to_string( profile.compute.pluginHosts ).c_str(), 0 );
                if( std::string own; !profile.compute.ownHost.empty() )
                    { for( const auto& id : profile.compute.ownHost ) own += ( own.empty() ? "" : "," ) + id; setenv( "OSCAIRN_OWN_HOST", own.c_str(), 0 ); }
                if( socket.isEmpty() && helper.empty() )
                    if( const auto missed = applyMemoryPolicy( -999 ); !missed.empty() && profile.reliability != "desktop" )
                        std::fprintf( stderr, "oscairn: %s\n", missed.c_str() );
                profile_ = profile; controllerProfiles_ = controllers; plugins_ = dir; nullAudio_ = audio.nullDevice; // for started()
                if( !offscreen ) showSplash( profile );
                // The engine starts, or is reached, off the message thread, so the splash stays
                // drawn; an in-process engine opens its devices on the message thread, so it starts there.
                const bool here = socket.isEmpty() && helper.empty();
                // RFC 0005: as in the separate engine, a plugin's preset listing runs in the scan
                // helper beside this program, not on this (the message) thread.
                if( const auto scan = executableFile().getSiblingFile( "oscairn-plugin-scan" );
                    here && placementPolicy().scan.empty() && scan.existsAsFile() )
                {
                    auto policy = placementPolicy();
                    policy.scan = scan.getFullPathName().toStdString();
                    setPlacementPolicy( policy );
                }
                starting_ = std::async( here ? std::launch::deferred : std::launch::async,
                    [this, plugins = dir.unquoted().toStdString(), path = socket.unquoted().toStdString(), helper, audio] {
                        struct Done { Application* app; ~Done() { juce::MessageManager::callAsync( [a = app] { a->started(); } ); } } done{ this };
                        return std::make_unique<Engine>( plugins, path, helper, audio );
                    } );
                if( here )
                    juce::MessageManager::callAsync( [this] { started(); } );
            }
            catch( const std::exception& e )
            {
                failed( e.what() );
            }
        }

        // The rest of starting, once the engine has started or failed to (called again by a
        // deferred start, which has nothing left to do then).
        void started()
        {
            if( !starting_.valid() )
                return;
            const auto args = juce::StringArray::fromTokens( commandLine_, true );
            const auto& profile = profile_;
            const auto& controllers = controllerProfiles_;
            const auto& dir = plugins_;
            try
            {
                engine_ = starting_.get();
                // RFC 0025: with the profile's memory classes, the device's memory less what is
                // reserved; the shell's base (and an in-process engine's) is this process's memory now.
                Control::Budget budget{ profile.cpuBudget(), profile.memoryBudget() };
                std::vector<Control::AppMemory> always;
                if( profile.memoryClasses() )
                {
                    constexpr uint64_t MiB = 1ull << 20;
                    budget.device = uint64_t( profile.compute.memory ) * MiB;
                    budget.system = uint64_t( profile.compute.osMemory ) * MiB + residentBytes();
                    budget.floor = uint64_t( profile.compute.performanceMemory ) * MiB;
                    budget.margin = profile.memoryMargin();
                    // An app that is a process, at its manifest's reservation; Oscairn's in-process apps at their placeholders.
                    const auto manifests = findApps( appsFolder() );
                    for( const auto& a : profile.alwaysAvailable )
                    {
                        const auto m = std::find_if( manifests.begin(), manifests.end(), [&]( const AppManifest& x ) { return x.id == a; } );
                        always.push_back( { a, ( m != manifests.end() ? m->reserve : uint64_t( Profile::appMemory( a ) ) ) * MiB } );
                    }
                    if( budget.system + budget.floor + budget.margin > budget.device )
                    {
                        std::fprintf( stderr, "oscairn: memory classes off: the OS, this app, the floor and the margin exceed the device's memory\n" );
                        budget.device = budget.system = budget.floor = budget.margin = 0; always.clear();
                    }
                }
                engine_->edit( [budget, always]( Control& c ) { c.setBudget( budget ); if( !always.empty() ) c.setAlwaysAvailable( always ); } );
                // RFC 0031: the package repositories its image ships, and whether it hides the official one.
                std::vector<Control::Repository> shipped;
                for( const auto& r : profile.repositories ) shipped.push_back( { r.address, r.name, r.key, "device" } );
                engine_->edit( [shipped, hide = profile.hideOfficial]( Control& c ) { c.setDeviceRepositories( shipped, hide ); } );
                // RFC 0020: the retroactive buffer Capture saves from, as long as was chosen (not for a snapshot).
                if( !args.contains( "--snapshot" ) )
                    if( const auto error = engine_->setCapture( readCaptureSeconds( theme_->configuration(), profile ) ); !error.empty() )
                        std::fprintf( stderr, "oscairn: capture: %s\n", error.c_str() );

                auto layouts = builtInLayouts();
                auto wanted  = argument( args, "--layout" );
                if( wanted.isEmpty() )
                    wanted = profile.screen() ? juce::String( profile.screen()->layout ) : juce::String( "desktop" );
                juce::String json;
                for( const auto& l : layouts )
                    if( l.name == wanted )
                        json = l.json;
                if( json.isEmpty() )
                {
                    const auto file = juce::File::getCurrentWorkingDirectory().getChildFile( wanted.unquoted() );
                    if( !file.existsAsFile() )
                        throw std::runtime_error( "no layout named " + wanted.toStdString() + " and no such file" );
                    json = file.loadFileAsString();
                }
                auto mode = readInputMode( theme_->configuration() );
                if( const auto m = argument( args, "--input-mode" ); m.isNotEmpty() )
                {
                    if( m == "automatic" ) mode = InputMode::Automatic;
                    else if( m == "mouse" ) mode = InputMode::Mouse;
                    else if( m == "touch" ) mode = InputMode::Touch;
                    else throw std::invalid_argument( "--input-mode must be automatic, mouse or touch" );
                }
                window_ = std::make_unique<Window>( *engine_, std::move( layouts ), Layout::parse( json ), profile,*theme_,juce::File(dir.unquoted()), mode, profileSource( args ) );
                // With --audio null this process opens no MIDI port either: its engine opens none.
                if( args.contains( "--snapshot" ) || nullAudio_ )
                    std::fprintf( stderr, "oscairn: 0 MIDI ports open in the app\n" );
                else
                    controllers_ = std::make_unique<Controllers>( *engine_, controlGroups( profile ), controllers );
                if( !args.contains( "--snapshot" ) )
                {
                    // The Performance last open, now the budget is set (RFC 0028, RFC 0025), off this thread; the
                    // engine remembers it. Only a separate engine opens Performances (not --in-process); --play
                    // plays the startup instrument's sequencer, which an open would stop.
                    if( engine_->canPrepareSounds() && !args.contains( "--play" ) ) { SoundAction reopen; reopen.kind = SoundAction::ReopenPerformance; engine_->soundAction( reopen ); }
                }
                if( const auto app = argument( args, "--app" ); app.isNotEmpty() ) window_->openApp( app.toLowerCase() );
                if( const auto view = argument( args, "--view" ); view.isNotEmpty() )
                {
                    DesktopShell::Tab tab;
                    if( !window_->shell() ) window_->openPerformance(); // from Home, as a tap on it would
                    if( !window_->shell() || !DesktopShell::tabNamed( view, tab ) )
                        throw std::invalid_argument( "--view needs the desktop layout and one of Part, Mixer, Routing, Controls, Play" );
                    window_->shell()->showTab( tab );
                }
                if( args.contains( "--play" ) )
                    engine_->setPlaying( true );

                if( const auto snapshot = argument( args, "--snapshot" ); snapshot.isNotEmpty() )
                {
                    const auto seconds = argument( args, "--play" ).getDoubleValue();
                    const auto size = juce::StringArray::fromTokens( argument( args, "--size" ), "x", "" );
                    const auto w = size.size() == 2 ? size[0].getIntValue() : 0, h = size.size() == 2 ? size[1].getIntValue() : 0;
                    juce::Timer::callAfterDelay( juce::roundToInt( juce::jmax( 0.5, seconds ) * 1000.0 ), [this, snapshot, w, h] {
                        writeSnapshot( juce::File::getCurrentWorkingDirectory().getChildFile( snapshot ), w, h );
                        quit();
                    } );
                }
                splash_.reset(); // the window is up
            }
            catch( const std::exception& e )
            {
                failed( e.what() );
            }
        }

        // Cmd-Q, the window's close button and the menu's Quit: a managed engine stops once let go, so
        // unsaved changes are asked about first. An engine that stays running keeps them; --snapshot
        // and a failed start quit without asking.
        void systemRequestedQuit() override
        {
            if( asking_ ) { asking_->toFront( true ); return; }
            if( window_ && engine_ && engine_->managed() ) asking_ = askToSave( *engine_, window_.get(), "quitting", [] { quit(); } );
            else quit();
        }

        void shutdown() override
        {
            if( starting_.valid() ) // quit while starting: the engine is let go here, once started
                try { starting_.get(); } catch( const std::exception& ) {}
            controllers_.reset(); // they play through the engine
            window_.reset(); // the panel uses the engine
            engine_.reset();
            splash_.reset();
            paints_.reset();
            theme_.reset();
        }

    private:
        // The splash screen until the window is up: the whole display on a device's panel, a
        // small window in the middle of the screen on the desktop.
        void showSplash( const Profile& profile )
        {
            splash_ = std::make_unique<Splash>();
            const auto* display = juce::Desktop::getInstance().getDisplays().getPrimaryDisplay();
            if( display == nullptr )
                return;
            const bool panel = profile.screen() && profile.screen()->surface != "desktop";
            if( panel ) capFrameRate(); // before its window is made, which reads it then
            splash_->setBounds( panel ? display->logicalBounds.toNearestInt() : display->userBounds.toNearestInt().withSizeKeepingCentre( 420, 300 ) );
            splash_->addToDesktop( panel ? 0 : juce::ComponentPeer::windowHasDropShadow );
            if( panel ) { fillScreen( *splash_ ); setDeviceScreen( splash_.get(), Profile::Screen::touchTarget ); } // a failure's alert opens inside it
            splash_->setVisible( true );
            splash_->toFront( true ); // over other apps' windows, as a starting app is
        }

        void failed( const juce::String& message )
        {
            std::fprintf( stderr, "oscairn-standalone: %s\n", message.toRawUTF8() );
            setApplicationReturnValue( 1 );
            if( splash_ )
                splash_->setStatus( message );
            if(juce::StringArray::fromTokens(commandLine_,true).contains("--snapshot"))quit();
            else juce::AlertWindow::showAsync(
                juce::MessageBoxOptions().withIconType(juce::MessageBoxIconType::WarningIcon)
                    .withTitle("Oscairn could not start").withMessage(message).withButton("Retry").withButton("Quit"),
                [this](int choice) {
                    if(choice==1)
                    {
                        controllers_.reset();
                        window_.reset(); // the panel uses the engine
                        engine_.reset();
                        splash_.reset();
                        setApplicationReturnValue(0);
                        initialise(commandLine_);
                    }
                    else quit();
                });
        }

        void writeSnapshot( const juce::File& file, int width, int height )
        {
            auto* content = window_->snapshotView();
            const auto& l = window_->layout();
            if( width > 0 && height > 0 )
                content->setSize( width, height );
            else
                content->setSize( l.width, l.height ); // one design unit per pixel
            const auto image = content->createComponentSnapshot( content->getLocalBounds(), true, 1.0f );
            // What the window system would have been asked for (none was: off screen), for desktop-launch.
            std::fprintf( stderr, "oscairn: top-level windows: %d; the main one %s\n", juce::TopLevelWindow::getNumTopLevelWindows(),
                          fillsScreen( *window_ ) && !window_->isUsingNativeTitleBar() && window_->getTitleBarHeight() == 0 && window_->getBorderThickness().isEmpty() ? "full screen, undecorated" : "a desktop window" );
            std::fprintf( stderr, "oscairn: the transport %s\n", engine_->status().playing ? "playing" : "stopped" );
            file.deleteFile();
            juce::FileOutputStream out( file );
            if( !out.openedOk() || !juce::PNGImageFormat().writeImageToStream( image, out ) )
            {
                std::fprintf( stderr, "oscairn-standalone: could not write %s\n", file.getFullPathName().toRawUTF8() );
                setApplicationReturnValue( 1 );
            }
        }

        std::unique_ptr<Theme> theme_;
        std::unique_ptr<PaintCounter> paints_;
        std::unique_ptr<Engine> engine_;
        std::unique_ptr<Window> window_;
        std::unique_ptr<Controllers> controllers_;
        std::unique_ptr<Splash> splash_;
        // Starting: the command line, what initialise found for started() and the engine on its way.
        juce::String commandLine_, plugins_;
        bool nullAudio_ = false; // --audio null: the engine opens no MIDI device, nor does this process
        Profile profile_;
        std::vector<Profile> controllerProfiles_;
        std::future<std::unique_ptr<Engine>> starting_;
        juce::Component::SafePointer<juce::Component> asking_; // the quit's prompt, while it shows
    };
}

START_JUCE_APPLICATION( oscairn::standalone::Application )
