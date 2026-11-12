// SPDX-License-Identifier: AGPL-3.0-or-later
// Installing from package repositories (RFC 0031): each job reads, fetches, checks and unpacks on
// a thread of its own, so the control thread never waits on a drive or the network; the control
// thread moves what it unpacked into place when the job is next asked about.
#include "oscairn/session.h"
#include "oscairn/package.h"
#include "oscairn/repository.h"
#include "oscairn/library.h"
#include "oscairn/signing.h"
#include "JsonInput.h"
#include "sheredom/json.h"
#include <algorithm>
#include <cctype>
#include <fstream>
#include <iterator>
#include <mutex>
#include <optional>
#include <random>
#include <thread>

namespace oscairn {
namespace {
namespace fs=std::filesystem;
fs::path knownFile(){return dataHome()/"repositories.json";}
std::mutex knownMutex; // repositories.json: written by job threads (serials seen) and the control thread (the list)

// repositories.json: the highest serial seen of each repository, by its root key and the ID its
// root gives, so an older root (from a stale or hostile mirror) is refused and a root claiming
// another repository's ID has serials of its own; the repositories the user added; and which are
// turned on or off, by address: {"format":"oscairn.repositories","version":2,
// "seen":{"<root key>":{"<repository>":<serial>}},"added":[{"address","name","key"}],"enabled":{"<address>":true|false}}.
// Version 1 kept serials by the repository ID alone ("seen":{"<repository>":<serial>}); those are
// read as `unkeyed` and kept until the next root read (see()).
struct Known { std::map<std::string,std::map<std::string,uint64_t>> seen; std::map<std::string,uint64_t> unkeyed; std::vector<Control::Repository> added; std::map<std::string,bool> enabled; };
std::string str(const json_value_s* v){if(!v || v->type!=json_type_string)return {};const auto* s=static_cast<const json_string_s*>(v->payload);return {s->string,s->string_size};}
bool serial(const json_value_s* v,uint64_t& out){
    if(v->type!=json_type_number)return false;
    const auto* n=static_cast<const json_number_s*>(v->payload);
    try{out=std::stoull(std::string(n->number,n->number_size));return true;}catch(const std::exception&){return false;}
}
std::string name(const json_object_element_s* m){return {m->name->string,m->name->string_size};}
Known readKnown(){
    Known out;
    std::error_code e;
    if(!fs::is_regular_file(knownFile(),e) || fs::file_size(knownFile(),e)>1024*1024)return out;
    std::ifstream in(knownFile(),std::ios::binary);
    const std::string bytes((std::istreambuf_iterator<char>(in)),std::istreambuf_iterator<char>());
    if(!detail::jsonPreflight(bytes,4).empty())return out;
    const std::unique_ptr<json_value_s,decltype(&std::free)> doc{json_parse(bytes.data(),bytes.size()),&std::free};
    const auto* seen=detail::member(doc.get(),"seen");
    uint64_t n=0;
    if(seen && seen->type==json_type_object)for(auto* m=static_cast<const json_object_s*>(seen->payload)->start;m;m=m->next){
        if(m->value->type==json_type_object){for(auto* r=static_cast<const json_object_s*>(m->value->payload)->start;r;r=r->next)if(serial(r->value,n))out.seen[name(m)][name(r)]=n;}
        else if(serial(m->value,n))out.unkeyed[name(m)]=n;
    }
    const auto* added=detail::member(doc.get(),"added");
    if(added && added->type==json_type_array)for(auto* a=static_cast<const json_array_s*>(added->payload)->start;a;a=a->next){
        Control::Repository r{str(detail::member(a->value,"address")),str(detail::member(a->value,"name")),str(detail::member(a->value,"key")),"user"};
        if(!r.address.empty() && !signing::keyId(r.key).empty())out.added.push_back(std::move(r));
    }
    const auto* enabled=detail::member(doc.get(),"enabled");
    if(enabled && enabled->type==json_type_object)for(auto* m=static_cast<const json_object_s*>(enabled->payload)->start;m;m=m->next)
        if(m->value->type==json_type_true || m->value->type==json_type_false)out.enabled[std::string(m->name->string,m->name->string_size)]=m->value->type==json_type_true;
    return out;
}
void writeKnown(const Known& k){
    std::string out="{\"format\":\"oscairn.repositories\",\"version\":2,\"seen\":{";
    bool first=true;
    for(const auto& [key,ids]:k.seen){
        out+=(first?"":",")+detail::quote(key)+":{";first=false;
        bool one=true;for(const auto& [id,n]:ids){out+=(one?"":",")+detail::quote(id)+":"+std::to_string(n);one=false;}
        out+='}';
    }
    for(const auto& [id,n]:k.unkeyed){out+=(first?"":",")+detail::quote(id)+":"+std::to_string(n);first=false;}
    out+="},\"added\":[";
    first=true;for(const auto& r:k.added){out+=(first?"":",")+std::string("{\"address\":")+detail::quote(r.address)+",\"name\":"+detail::quote(r.name)+",\"key\":"+detail::quote(r.key)+'}';first=false;}
    out+="],\"enabled\":{";
    first=true;for(const auto& [address,on]:k.enabled){out+=(first?"":",")+detail::quote(address)+":"+(on?"true":"false");first=false;}
    out+="}}\n";
    writeAtomically(knownFile(),out);
}
fs::path kept(){return dataHome()/"Repositories";} // each repository's index, read last, in a folder per root key
// Refuses a root older than one seen from this root key under this repository ID, and remembers
// this one. Serials kept by ID alone (version 1) are first given to every key trusted for
// repositories, as any of them could have signed one, so no rollback protection is lost; the
// indexes kept by ID alone go, being copies the next listing reads again whole.
void see(const std::string& rootKey,const std::string& repository,uint64_t serial,const std::vector<std::string>& trusted){
    std::lock_guard lock(knownMutex);
    auto known=readKnown();
    if(!known.unkeyed.empty()){
        for(const auto& [id,n]:known.unkeyed)for(const auto& key:trusted){auto& s=known.seen[key][id];s=std::max(s,n);}
        known.unkeyed.clear();
        std::error_code e;
        std::vector<fs::path> old;
        for(fs::directory_iterator i(kept(),e),end;!e && i!=end;i.increment(e))if(i->is_regular_file(e))old.push_back(i->path());
        for(const auto& f:old)fs::remove(f,e);
    }
    auto& seen=known.seen[rootKey];
    if(seen.count(repository) && serial<seen[repository])
        throw std::runtime_error("not a usable repository: its index (serial "+std::to_string(serial)+") is older than one this device has seen ("+std::to_string(seen[repository])+"); a mirror may be out of date or replaying an old index");
    seen[repository]=serial;
    writeKnown(known);
}
// Oscairn's own repository: known to every device, its root key trusted for repositories only.
const Control::Repository official{"https://packages.oscairn.org","Oscairn","6a7eceba0a34104fea44e99e8c904fc53f410b4ad7ee615bbb51bf6876630841","official",true};
bool web(const std::string& a){return a.rfind("https://",0)==0 || a.rfind("http://",0)==0;}
// An address as it is kept: a web address without '/' at its end.
std::string addressOf(std::string a){
    if(web(a))while(!a.empty() && a.back()=='/')a.pop_back();
    return a;
}
// A repository's address, root key and name, checked as they come in; the name defaults to its fingerprint.
Control::Repository checked(const Control::Repository& in,const std::string& origin){
    Control::Repository r{addressOf(in.address),in.name,in.key,origin,true};
    if(r.address.empty() || r.address.size()>4096 || std::any_of(r.address.begin(),r.address.end(),[](char c){return static_cast<unsigned char>(c)<0x20;}) ||
       (web(r.address) ? r.address.find("://")+3>=r.address.size() : !fs::path(r.address).is_absolute()))
        throw std::invalid_argument("a repository's address is its https:// (or http://) address, or a folder's full path");
    if(signing::keyId(r.key).empty())throw std::invalid_argument("a repository's root key is 64 lowercase hex digits");
    if(r.name.size()>256)throw std::invalid_argument("a repository's name is at most 256 bytes");
    if(r.name.empty())r.name="Repository "+signing::keyId(r.key);
    return r;
}
// A repository on the web (https:// or http://), or in a folder.
std::unique_ptr<repository::Source> sourceFor(const std::string& source,const std::atomic<bool>* cancel){
    if(source.empty() || source.size()>4096)throw std::invalid_argument("a repository is a folder holding repository.json, or its https:// address");
    if(source.rfind("https://",0)==0 || source.rfind("http://",0)==0)return repository::web(source,cancel);
    return repository::folder(source);
}
fs::path scratch(const char* suffix){std::random_device random;return dataHome()/(".install-"+std::to_string(random())+std::to_string(random())+suffix);}
// The repository this device knows a source as: by its address, else, for a folder (a copy on a
// drive), by its verified root key; null when it knows none.
const Control::Repository* knownAs(const std::vector<Control::Repository>& known,const std::string& source,const std::string& rootKey){
    for(const auto& r:known)if(r.address==addressOf(source))return &r;
    if(!web(source) && !rootKey.empty())for(const auto& r:known)if(r.key==rootKey)return &r;
    return nullptr;
}
// The only root keys a source may have: a known repository's address, the key recorded for it;
// the snapshot that comes with the app, a shipped repository's (RFC 0031: a copy of the official
// one, or of a device image's); anything else (a drive, an address being added), any key trusted
// for repositories.
std::vector<std::string> recordedKeys(const std::vector<Control::Repository>& known,const std::vector<Control::Repository>& ships,const std::string& source,const fs::path& snapshot){
    for(const auto& r:known)if(r.address==addressOf(source))return {r.key};
    std::vector<std::string> out;
    if(!snapshot.empty() && source==snapshot.string())for(const auto& s:ships)out.push_back(s.key);
    return out;
}
// Each repository's revocation list as last read, beside its kept index:
// Repositories/<root key>/<repository>.revocations.json, written only once see() has taken the
// root's serial, so an older root never replaces it.
void keepRevocations(const repository::Index& index){
    if(!repository::plainId(index.repository))return;
    std::lock_guard lock(knownMutex);
    std::error_code e;
    fs::create_directories(kept()/index.rootKey,e);
    writeAtomically(kept()/index.rootKey/(index.repository+".revocations.json"),index.revocationsJson);
}
// The lists last read from repositories with this root key, one per repository ID its roots gave.
// One that no longer reads (damaged on disk) is left out until the next listing keeps it again.
std::vector<repository::Revocations> keptRevocations(const std::string& rootKey){
    std::vector<repository::Revocations> out;
    if(signing::keyId(rootKey).empty())return out; // a path from a key: 64 hex digits only
    std::lock_guard lock(knownMutex);
    std::error_code e;
    for(fs::directory_iterator i(kept()/rootKey,e),end;!e && i!=end;i.increment(e)){
        const auto file=i->path();
        if(!file.filename().string().ends_with(".revocations.json") || fs::file_size(file,e)>(4u<<20) || e)continue;
        std::ifstream in(file,std::ios::binary);
        try{out.push_back(repository::revocations({std::istreambuf_iterator<char>(in),std::istreambuf_iterator<char>()}));}catch(const std::runtime_error&){}
    }
    return out;
}
// The revocation lists that apply to a release (RFC 0031): those of the repository it is from
// (`own`, by this device's name for it), for its version, a key that signed it or a build in it;
// and the official repository's, for a key or a build, as the official list speaks for every
// repository. Another repository's list speaks only for its own packages, so one the user added
// cannot withdraw what the official one or another lists.
struct Lists { std::vector<repository::Revocations> own, official; std::string ownName, officialName; };
// Why a release is withdrawn by them, or an empty reason: signers are key IDs, builds digests.
Control::Withdrawal withdrawal(const Lists& l,const std::string& id,const std::string& version,const std::vector<std::string>& signers,const std::vector<std::string>& builds={}){
    auto by=[&](const std::vector<repository::Revocations>& lists,const std::string& name,bool versions)->std::optional<Control::Withdrawal>{
        for(const auto& r:lists){
            if(versions)for(const auto& v:r.versions)if(v.id==id && v.version==version)return Control::Withdrawal{name,v.reason,v.date};
            for(const auto& k:r.keys)if(std::count(signers.begin(),signers.end(),signing::keyId(k.key)))return Control::Withdrawal{name,k.reason,k.date};
            for(const auto& b:r.builds)if(std::count(builds.begin(),builds.end(),b.sha256))return Control::Withdrawal{name,b.reason,b.date};
        }
        return std::nullopt;
    };
    if(auto w=by(l.own,l.ownName,true))return *w;
    if(auto w=by(l.official,l.officialName,false))return *w;
    return {};
}
std::string withdrawnSaying(const std::string& id,const std::string& version,const Control::Withdrawal& w){return id+" "+version+" was withdrawn by "+w.repository+": "+w.reason;}
}

struct Session::PackageWork {
    std::mutex mutex;
    uint64_t id=0;           // set before the thread starts
    PackageJob job;          // guarded by mutex
    bool unpacked=false;     // the thread is done and left packages, each in its folder, to move into place
    std::vector<std::pair<Package,fs::path>> packages; // the asset libraries it needs first, then the package
    std::map<std::string,std::string> archives; // each one's archive SHA-256 as the index lists it, by package ID
    bool acceptUnsignedCode=false;
    bool replace=false;      // the user chose the repository's build over a different one installed
    std::string source;      // the repository's address, recorded with what it installs
    std::atomic<bool> cancel=false; // set when the session goes: a download stops
    std::thread thread;
    ~PackageWork(){cancel=true;if(thread.joinable())thread.join();for(const auto& [p,into]:packages){std::error_code e;fs::remove_all(into,e);}}
};

namespace {
// The trust store's keys as the job needs them, read on the control thread before it starts.
void keys(const std::vector<Control::TrustedKey>& all,std::vector<std::string>& trusted,std::vector<std::string>& revoked){
    for(const auto& k:all){
        if(k.revoked)revoked.push_back(k.key);
        else if(std::find(k.scope.begin(),k.scope.end(),"repositories")!=k.scope.end())trusted.push_back(k.key);
    }
}
}

std::vector<Control::Repository> Session::shipped() const{
    auto out=deviceRepositories_;
    out.insert(out.begin(),official);
    return out;
}

std::vector<Control::Repository> Session::repositories(){
    Known known;
    {std::lock_guard lock(knownMutex);known=readKnown();}
    auto out=shipped();
    out.front().enabled=!hideOfficial_;
    out.insert(out.end(),known.added.begin(),known.added.end());
    for(auto& r:out)if(const auto e=known.enabled.find(r.address);e!=known.enabled.end())r.enabled=e->second;
    // Names as everything shows them: one read like an earlier one's (a repository the user added
    // calling itself "Oscairn", say) is told apart by its key, or by its address where the key is the same.
    auto plain=[](const std::string& s){std::string o;for(const char c:s)if(!std::isspace(static_cast<unsigned char>(c)))o+=char(std::tolower(static_cast<unsigned char>(c)));return o;};
    for(size_t i=0;i<out.size();++i)for(size_t j=0;j<i;++j)if(plain(out[j].name)==plain(out[i].name)){
        out[i].name+=" ("+(out[j].key==out[i].key?out[i].address:"key "+signing::keyId(out[i].key))+")";
        j=size_t(-1); // compared again from the first, as the new name may read like another's
    }
    return out;
}

void Session::addRepository(const Repository& in){
    auto r=checked(in,"user");
    const auto ships=shipped();
    if(std::any_of(ships.begin(),ships.end(),[&](const auto& s){return s.address==r.address;}))throw std::invalid_argument(r.address+" is listed already, as this device ships it");
    // Its root key, trusted for repositories beside whatever else it is trusted for; a revoked one is refused.
    if(std::none_of(ships.begin(),ships.end(),[&](const auto& s){return s.key==r.key;})){
        TrustedKey k{"",r.key,r.name,{"repositories"}};
        for(const auto& t:trustedKeys())if(t.key==r.key){
            if(t.revoked)throw std::invalid_argument("the repository's root key "+t.id+" is revoked on this device");
            k=t;k.scope.push_back("repositories");
        }
        setTrustedKey(k);
    }
    std::lock_guard lock(knownMutex);
    auto known=readKnown();
    std::erase_if(known.added,[&](const auto& a){return a.address==r.address;});
    known.added.push_back(r);
    known.enabled.erase(r.address);
    writeKnown(known);
}

void Session::removeRepository(const std::string& address){
    const auto at=addressOf(address);
    const auto ships=shipped();
    if(std::any_of(ships.begin(),ships.end(),[&](const auto& s){return s.address==at;}))
        throw std::invalid_argument(at==official.address?"the official repository can be turned off but not removed":"a repository this device ships can be turned off but not removed");
    std::string key;
    bool shared=false;
    {
        std::lock_guard lock(knownMutex);
        auto known=readKnown();
        const auto gone=std::find_if(known.added.begin(),known.added.end(),[&](const auto& a){return a.address==at;});
        if(gone==known.added.end())throw std::invalid_argument("no repository "+at+" on this device");
        key=gone->key;
        known.added.erase(gone);
        known.enabled.erase(at);
        writeKnown(known);
        shared=std::any_of(known.added.begin(),known.added.end(),[&](const auto& a){return a.key==key;}) || std::any_of(ships.begin(),ships.end(),[&](const auto& s){return s.key==key;});
    }
    // Its root key is no longer trusted for repositories, unless another listed repository has it.
    if(!shared)for(auto t:trustedKeys())if(t.key==key && std::erase(t.scope,std::string("repositories"))){
        if(t.scope.empty() && !t.revoked)removeTrustedKey(t.id);else setTrustedKey(t);
    }
}

void Session::setRepositoryEnabled(const std::string& address,bool enabled){
    const auto at=addressOf(address);
    const auto all=repositories();
    if(std::none_of(all.begin(),all.end(),[&](const auto& r){return r.address==at;}))throw std::invalid_argument("no repository "+at+" on this device");
    std::lock_guard lock(knownMutex);
    auto known=readKnown();
    known.enabled[at]=enabled;
    writeKnown(known);
}

void Session::setDeviceRepositories(const std::vector<Repository>& list,bool hideOfficial){
    std::vector<Repository> out;
    for(const auto& in:list){
        auto r=checked(in,"device");
        if(r.address==official.address || std::any_of(out.begin(),out.end(),[&](const auto& o){return o.address==r.address;}))throw std::invalid_argument(r.address+" is listed twice");
        out.push_back(std::move(r));
    }
    deviceRepositories_=std::move(out);hideOfficial_=hideOfficial;
}

uint64_t Session::listRepository(const std::string& source){
    if(packageWork_ && packageJob(packageWork_->id).state=="running")throw std::logic_error("a package job is running; wait for it to finish");
    std::vector<std::string> trusted,revoked;keys(trustedKeys(),trusted,revoked);
    const auto known=repositories(); // read on the control thread: the names this device gives them, and their keys
    const auto expected=recordedKeys(known,shipped(),source,preinstallFolder_);
    auto work=std::make_shared<PackageWork>();
    work->id=work->job.id=++packageJobs_;work->job.state="running";
    auto* w=work.get();
    packageWork_.reset(); // the last job's thread is done; joined here
    w->thread=std::thread([w,source,trusted,revoked,known,expected,officialName=known.front().name]{
        PackageJob out;
        try{
            auto from=sourceFor(source,&w->cancel);
            const auto index=repository::open(*from,trusted,revoked,out.rootKey,out.claimedName,kept(),expected);
            see(index.rootKey,index.repository,index.serial,trusted);
            keepRevocations(index);
            if(const auto* r=knownAs(known,source,index.rootKey))out.name=r->name;
            out.repository=index.repository;out.serial=index.serial;
            const Lists lists{{index.revoked},index.rootKey==official.key?std::vector<repository::Revocations>{}:keptRevocations(official.key),out.name.empty()?addressOf(source):out.name,officialName};
            for(const auto& p:index.packages){
                RepositoryPackage listed{p.id,p.name,p.summary,p.license,p.publisherName,p.publisherKey,p.tags,{}};
                for(const auto& v:p.versions)listed.versions.push_back({v.version,v.channel,v.yanked,repository::incompatible(v,OSCAIRN_VERSION,platform()),v.archiveSha256,v.size,v.installed,v.types,v.dependencies,
                                                                        withdrawal(lists,p.id,v.version,{p.publisherKey})});
                out.packages.push_back(std::move(listed));
            }
            out.state="done";
        }catch(const std::exception& e){out.state="failed";out.error=e.what();}
        out.id=w->id;std::lock_guard lock(w->mutex);w->job=std::move(out);
    });
    packageWork_=std::move(work);
    return packageJobs_;
}

uint64_t Session::installFromRepository(const std::string& source,const std::string& id,const std::string& version,bool acceptUnsignedCode,bool replace){
    if(packageWork_ && packageJob(packageWork_->id).state=="running")throw std::logic_error("a package job is running; wait for it to finish");
    if(id.empty())throw std::invalid_argument("which package?");
    std::vector<std::string> trusted,revoked;keys(trustedKeys(),trusted,revoked);
    auto work=std::make_shared<PackageWork>();
    work->id=work->job.id=++packageJobs_;work->job.state="running";work->acceptUnsignedCode=acceptUnsignedCode;work->replace=replace;work->source=addressOf(source);
    auto* w=work.get();
    packageWork_.reset();
    const auto assetRoot=assets_.root(); // read on the control thread; the job reads which libraries are installed from its own Store
    const auto ships=shipped();
    const auto known=repositories();
    const auto expected=recordedKeys(known,ships,source,preinstallFolder_);
    w->thread=std::thread([w,source,id,version,trusted,revoked,assetRoot,ships,known,expected,officialName=known.front().name]{
        std::string rootKey,error,chosen,repositoryId,name,claimed;uint64_t serial=0;
        std::vector<std::pair<Package,fs::path>> unpacked; // the package, then each asset library it needs
        std::map<std::string,std::string> archives;
        try{
            auto from=sourceFor(source,&w->cancel);
            const auto index=repository::open(*from,trusted,revoked,rootKey,claimed,kept(),expected);
            see(index.rootKey,index.repository,index.serial,trusted);
            keepRevocations(index);
            repositoryId=index.repository;serial=index.serial;
            if(const auto* r=knownAs(known,source,index.rootKey))name=r->name;
            const auto called=name.empty()?addressOf(source):name; // never the name its root claims
            const Lists lists{{index.revoked},index.rootKey==official.key?std::vector<repository::Revocations>{}:keptRevocations(official.key),called,officialName};
            // A release: fetched (or read in place), checked against the signed index and unpacked.
            auto unpack=[&](const repository::Package& package,const repository::Version& pick){
                // The package file: read in place from a folder, else fetched into the data folder,
                // where a download cut short stays to be resumed; either way it must be the size and
                // SHA-256 the signed index gives.
                const auto blob=repository::blobPath(pick.archiveSha256);
                auto file=from->local(blob);
                fs::path download;
                if(file.empty()){download=dataHome()/(".install-"+pick.archiveSha256+".partial");from->fetch(pick.archiveSha256,pick.archiveSize,download);file=download;}
                struct Gone{fs::path p;~Gone(){std::error_code e;if(!p.empty())fs::remove(p,e);}} gone{download}; // whole: used or refused; one cut short is kept to resume
                std::error_code e;
                if(!fs::is_regular_file(file,e) || fs::file_size(file,e)!=pick.archiveSize || assets::sha256File(file)!=pick.archiveSha256)
                    throw std::runtime_error("refused: the package file for "+package.id+" "+pick.version+" is missing, or not the one the index names (changed or damaged): "+blob);
                unpacked.push_back({Package{},scratch("")});
                auto& [got,into]=unpacked.back();
                got=readPackage(file,into); // every hash and signature in it, and room for it
                // It is the release the index names, from the publisher it names.
                if(got.manifestSha256!=pick.manifestSha256 || got.id!=package.id)
                    throw std::runtime_error("refused: the package file for "+package.id+" "+pick.version+" is not the release the index names");
                if(got.signature.key.empty() || signing::keyId(got.signature.key)!=package.publisherKey)
                    throw std::runtime_error("refused: "+package.id+" "+pick.version+" is not signed by its publisher's key "+package.publisherKey);
                // Withdrawn by what it holds too: a key that signed a build in it, or a build by its digest.
                std::vector<std::string> signers{package.publisherKey},builds;
                for(const auto& plugin:got.plugins)for(const auto& b:plugin.builds){builds.push_back(b.sha256);for(const auto& s:b.signatures)signers.push_back(signing::keyId(s.key));}
                if(const auto w=withdrawal(lists,package.id,pick.version,signers,builds);!w.reason.empty())throw std::runtime_error("refused: "+withdrawnSaying(package.id,pick.version,w));
                archives[package.id]=pick.archiveSha256;
            };
            // The newest stable version this device can install that is neither yanked nor withdrawn, and at least `least`.
            auto newest=[&](const repository::Package& package,const std::string& least){
                const repository::Version* pick=nullptr;
                for(const auto& v:package.versions)
                    if(v.yanked.empty() && (v.channel.empty() || v.channel=="stable") && repository::incompatible(v,OSCAIRN_VERSION,platform()).empty() &&
                       withdrawal(lists,package.id,v.version,{package.publisherKey}).reason.empty() &&
                       (least.empty() || !repository::older(v.version,least)) && (!pick || repository::older(pick->version,v.version)))pick=&v;
                return pick;
            };
            const auto package=std::find_if(index.packages.begin(),index.packages.end(),[&](const auto& p){return p.id==id;});
            if(package==index.packages.end())throw std::runtime_error("no package "+id+" in "+called);
            // The version named (a yanked one too: a lock that names it still gets it), else the newest.
            const repository::Version* pick=version.empty()?newest(*package,{}):nullptr;
            if(!version.empty())for(const auto& v:package->versions)if(v.version==version)pick=&v;
            if(!pick && version.empty())for(const auto& v:package->versions)if(!pick || repository::older(pick->version,v.version))pick=&v; // none fits: say why the newest does not
            if(!pick)throw std::runtime_error(version.empty()?"no version of "+id+" to install":"no version "+version+" of "+id);
            // A withdrawn version is never installed, even named: unlike a yanked one, which a lock still gets.
            if(const auto w=withdrawal(lists,id,pick->version,{package->publisherKey});!w.reason.empty())throw std::runtime_error("refused: "+withdrawnSaying(id,pick->version,w));
            if(const auto why=repository::incompatible(*pick,OSCAIRN_VERSION,platform());!why.empty())throw std::runtime_error(id+" "+pick->version+" cannot be installed here: "+why);
            chosen=pick->version;
            unpack(*package,*pick);
            // The asset libraries it needs that this device lacks, or has too old, from the same repository.
            const auto needs=unpacked.front().first.assetDependencies;
            const assets::Store installed(assetRoot);
            for(const auto& d:needs){
                if(!installed.libraryRoot(d.id,d.version).empty())continue;
                const auto library=std::find_if(index.packages.begin(),index.packages.end(),[&](const auto& p){return p.id==d.id;});
                const auto* release=library==index.packages.end()?nullptr:newest(*library,d.version);
                if(!release)throw std::runtime_error(id+" "+chosen+" needs the asset library "+d.id+" "+d.version+" or later, which "+called+" does not have");
                unpack(*library,*release);
                if(unpacked.back().first.kind!="asset")throw std::runtime_error("refused: "+d.id+" is not an asset library");
            }
        }catch(const std::exception& x){error=x.what();for(const auto& [p,into]:unpacked){std::error_code e;fs::remove_all(into,e);}unpacked.clear();}
        std::lock_guard lock(w->mutex);
        // A folder holding a copy of a repository this device ships (a USB drive, or the packages
        // that come with the app) is recorded as that repository, where updates come from.
        if(!web(source))for(const auto& s:ships)if(!rootKey.empty() && s.key==rootKey)w->source=s.address;
        w->job.rootKey=rootKey;w->job.repository=repositoryId;w->job.name=name;w->job.claimedName=claimed;w->job.serial=serial;w->job.package=id;w->job.version=chosen;
        if(!error.empty()){w->job.state="failed";w->job.error=error;return;}
        std::rotate(unpacked.begin(),unpacked.begin()+1,unpacked.end()); // the libraries first, so the package finds them installed
        w->packages=std::move(unpacked);w->archives=std::move(archives);w->unpacked=true;
    });
    packageWork_=std::move(work);
    return packageJobs_;
}

// What is installed and withdrawn since (RFC 0031), by the lists last read: said, never acted on.
void Session::markWithdrawn(std::vector<InstalledPackage>& packages){
    const auto known=repositories();
    const Lists base{{},keptRevocations(official.key),{},known.front().name};
    std::map<std::string,std::vector<repository::Revocations>> byKey;
    for(auto& p:packages){
        auto lists=base;
        for(const auto& r:known)if(r.address==p.repository){
            if(!byKey.count(r.key))byKey[r.key]=keptRevocations(r.key);
            lists.own=byKey[r.key];lists.ownName=r.name;break;
        }
        p.withdrawn=withdrawal(lists,p.id,p.version,p.signers);
    }
}

Control::PackageJob Session::packageJob(uint64_t id){
    if(!packageWork_ || packageWork_->id!=id)throw std::invalid_argument("no package job "+std::to_string(id));
    auto& w=*packageWork_;
    bool unpacked=false;
    {std::lock_guard lock(w.mutex);unpacked=w.unpacked;w.unpacked=false;}
    if(unpacked){
        // Moved into place here, on the control thread, which owns the library and the store: renames, not copies.
        if(w.thread.joinable())w.thread.join();
        auto packages=std::move(w.packages);w.packages.clear();
        PackageJob done;done.state="done";
        // The package's code is acceptable before anything goes in: a refusal installs none of the libraries it needs either.
        try{installUnpacked(packages.back().first,packages.back().second,true,{},w.acceptUnsignedCode,true,true,false,w.replace);}
        catch(const CodeRefused& e){done.state="failed";done.error=e.what();done.installed.code=e.code;} // its signers' keys, to trust
        catch(const std::exception& e){done.state="failed";done.error=e.what();}
        // Each asset library it needs, then the package; installUnpacked removes each folder either way.
        for(auto& [package,into]:packages){
            if(done.state=="failed"){std::error_code e;fs::remove_all(into,e);continue;}
            try{
                const bool dependency=&package!=&packages.back().first; // a library the package needs, installed first
                const auto installed=installUnpacked(package,into,true,{},w.acceptUnsignedCode,true,false,dependency,w.replace && !dependency);
                recordPackage(package,true,w.job.version,w.source,w.archives[package.id],installed.otherBuild); // an asset library is recorded at its own version
                done.installed.added+=installed.added;done.installed.skipped+=installed.skipped;
                done.installed.notes.insert(done.installed.notes.end(),installed.notes.begin(),installed.notes.end());
                done.installed.code.insert(done.installed.code.end(),installed.code.begin(),installed.code.end());
            }catch(const std::exception& e){done.state="failed";done.error=e.what();}
        }
        std::lock_guard lock(w.mutex);w.job.state=done.state;w.job.error=done.error;w.job.installed=done.installed;
    }
    std::lock_guard lock(w.mutex);
    return w.job;
}
}
