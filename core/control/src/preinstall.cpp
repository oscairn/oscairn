// SPDX-License-Identifier: AGPL-3.0-or-later
// Preinstalled packages (RFC 0031): a release ships a repository snapshot (the official
// repository's signed root and index, and the archives of the packages it preinstalls), which the
// engine installs from at its first start through the normal package jobs, offline. Control
// thread only: started by the host, moved on by service() and by asking how it goes.
#include "oscairn/session.h"
#include "oscairn/library.h"
#include "oscairn/repository.h"
#include "JsonInput.h"
#include "sheredom/json.h"
#include <algorithm>
#include <fstream>

namespace oscairn {
namespace {
namespace fs=std::filesystem;
fs::path recordFile(){return dataHome()/"preinstalled.json";}

// preinstalled.json: each release that ran its preinstall, and each package any of them
// installed or found installed, with the release: {"format":"oscairn.preinstalled","version":1,
// "releases":["<release>"],"packages":[{"id","version","release"}]}. A package listed here is
// never preinstalled again, so one the user removed stays removed.
struct Record { std::vector<std::string> releases; std::vector<Control::PreinstalledPackage> packages; };
std::string str(const json_value_s* v){if(!v || v->type!=json_type_string)return {};const auto* s=static_cast<const json_string_s*>(v->payload);return {s->string,s->string_size};}
Record readRecord(){
    Record out;
    std::error_code e;
    if(!fs::is_regular_file(recordFile(),e) || fs::file_size(recordFile(),e)>1024*1024)return out;
    std::ifstream in(recordFile(),std::ios::binary);
    const std::string bytes((std::istreambuf_iterator<char>(in)),std::istreambuf_iterator<char>());
    if(!detail::jsonPreflight(bytes,4).empty())return out;
    const std::unique_ptr<json_value_s,decltype(&std::free)> doc{json_parse(bytes.data(),bytes.size()),&std::free};
    auto each=[&](const char* key,auto f){if(const auto* a=detail::member(doc.get(),key);a && a->type==json_type_array)for(auto* x=static_cast<const json_array_s*>(a->payload)->start;x;x=x->next)f(x->value);};
    each("releases",[&](const json_value_s* v){out.releases.push_back(str(v));});
    each("packages",[&](const json_value_s* v){out.packages.push_back({str(detail::member(v,"id")),{},str(detail::member(v,"version")),str(detail::member(v,"release")),{}});});
    return out;
}
void writeRecord(const Record& r){
    std::string releases,packages;
    for(const auto& x:r.releases)releases+=(releases.empty()?"":",")+detail::quote(x);
    for(const auto& p:r.packages)packages+=(packages.empty()?"":",")+std::string("{\"id\":")+detail::quote(p.id)+",\"version\":"+detail::quote(p.version)+",\"release\":"+detail::quote(p.state)+'}';
    writeAtomically(recordFile(),"{\"format\":\"oscairn.preinstalled\",\"version\":1,\"releases\":["+releases+"],\"packages\":["+packages+"]}\n");
}
// Remembers a package as preinstalled by this release (its state field holds the release).
void rememberPreinstalled(const Control::PreinstalledPackage& p,const std::string& release){
    auto r=readRecord();
    if(std::none_of(r.packages.begin(),r.packages.end(),[&](const auto& x){return x.id==p.id;}))r.packages.push_back({p.id,{},p.version,release,{}});
    writeRecord(r);
}
}

void Session::preinstall(const fs::path& folder){
    std::error_code e;
    if(folder.empty() || !fs::is_regular_file(folder/"repository.json",e))return;
    preinstall_={"none",OSCAIRN_RELEASE,{},0,{}};
    const auto record=readRecord();
    if(std::count(record.releases.begin(),record.releases.end(),preinstall_.release))return; // this release ran it already
    preinstallFolder_=folder;
    preinstall_.state="running";
    try{preinstall_.job=listRepository(folder.string());} // first the snapshot's index, checked as any repository's
    catch(const std::exception& x){preinstall_.error="the packages that come with Oscairn could not be read: "+std::string(x.what());} // finished, and recorded, by the next step
    advancePreinstall();
}

Control::Preinstall Session::preinstalled(){
    advancePreinstall();
    return preinstall_.state.empty()?Preinstall{"none",OSCAIRN_RELEASE,{},0,{}}:preinstall_;
}

void Session::advancePreinstall(){
    auto& s=preinstall_;
    if(s.state!="running")return;
    auto finish=[&]{
        s.state="done";s.job=0;
        auto r=readRecord();
        if(!std::count(r.releases.begin(),r.releases.end(),s.release))r.releases.push_back(s.release);
        writeRecord(r);
        std::string failed;
        for(const auto& p:s.packages)if(p.state=="failed")failed+=(failed.empty()?"":"; ")+p.error;
        if(!failed.empty() && s.error.empty())s.error=failed+". The Library's Catalog can install it.";
    };
    auto installed=[&](const std::string& id){const auto all=installedPackages();return std::any_of(all.begin(),all.end(),[&](const auto& p){return p.id==id;});};
    if(s.job){
        PackageJob job;
        try{job=packageJob(s.job);}
        catch(const std::invalid_argument&){job.state="lost";} // another job started and took its place after it ended
        if(job.state=="running")return;
        s.job=0;
        const auto at=std::find_if(s.packages.begin(),s.packages.end(),[](const auto& p){return p.state=="installing";});
        if(at==s.packages.end()){
            // The listing: each package whose archive the snapshot holds, at that version.
            if(job.state!="done"){s.error="the packages that come with Oscairn could not be read: "+job.error;finish();return;}
            const auto record=readRecord();
            for(const auto& p:job.packages){
                const RepositoryVersion* pick=nullptr;
                std::error_code e;
                for(const auto& v:p.versions)
                    if(fs::is_regular_file(preinstallFolder_/repository::blobPath(v.sha256),e) && (!pick || repository::older(pick->version,v.version)))pick=&v;
                if(!pick)continue;
                PreinstalledPackage out{p.id,p.name,pick->version,"waiting",{}};
                const bool before=std::any_of(record.packages.begin(),record.packages.end(),[&](const auto& x){return x.id==p.id;});
                // The user's own wins: installed at any version, it is left as it is and updates the normal way.
                if(installed(p.id)){out.state="kept";if(!before)rememberPreinstalled(out,s.release);}
                else if(before)out.state="removed";
                else if(!pick->incompatible.empty()){out.state="failed";out.error=p.name+" was not set up: "+pick->version+" cannot be installed here: "+pick->incompatible;}
                s.packages.push_back(std::move(out));
            }
        }else if(job.state=="done" || (job.state=="lost" && installed(at->id))){at->state="installed";rememberPreinstalled(*at,s.release);}
        else{
            at->state="failed";
            // Refused code is said plainly here: nobody at first start asked to accept it, and nothing does.
            const bool untrusted=std::any_of(job.installed.code.begin(),job.installed.code.end(),[](const auto& c){return c.status!="trusted";});
            at->error=job.state=="lost"?at->name+" was not set up: its install was interrupted"
                     :untrusted?at->name+" was not set up: its signature is not from a publisher this device trusts"
                     :at->name+" was not set up: "+job.error;
        }
    }
    // The next one waiting, once no other client's package job runs; one an earlier install
    // brought (an asset library a plugin needs) is installed already.
    if(packageWork_ && packageJob(packageJobs_).state=="running")return;
    for(auto& p:s.packages){
        if(p.state!="waiting")continue;
        if(installed(p.id)){p.state="installed";rememberPreinstalled(p,s.release);continue;}
        try{s.job=installFromRepository(preinstallFolder_.string(),p.id,p.version);p.state="installing";return;}
        catch(const std::exception& x){p.state="failed";p.error=p.name+" was not set up: "+x.what();}
    }
    finish();
}
}
