// SPDX-License-Identifier: AGPL-3.0-or-later
// The engine core's reference outputs, recorded (core/engine/tests/vectors/*.vec). A test hands every value it
// compares to a Tape, in order; the tape folds them into one running 64-bit digest per plan and, at each block's
// end, records or checks its top 16 bits, and every 64th block's and the plan's last block's whole digest. The
// running digest covers everything since the plan began, so the first block whose 16 bits differ is the first that
// differs (one chance in 65,536 it is a block later), and the 64-bit ones leave a difference nowhere to hide.
// A file is a text header (generator, seeds, sizes, the reference's commit, the floating-point conditions), a line
// "binary", then per plan its blocks' 16-bit digests and its checkpoints' 64-bit ones, little-endian.
// Live: in memory, each value's own digest, so the first value that differs is named (the C++ reference's
// run is recorded, then the Rust's checked against it).
#pragma once
#include <bit>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

static_assert(std::endian::native==std::endian::little,"the vector files are little-endian, as every platform Oscairn builds for");
extern "C" uint64_t oscairn_vectors_math_calls[3]; // vectors_math.c: exp, log1p, pow

namespace vectors
{
inline uint64_t mix(uint64_t h,uint64_t w) { h^=w*0x9E3779B97F4A7C15ull;return (h<<31|h>>33)*0xC2B2AE3D27D4EB4Full; }
inline uint64_t finish(uint64_t h) { h^=h>>33;h*=0xff51afd7ed558ccdull;h^=h>>33;h*=0xc4ceb9fe1a85ec53ull;return h^h>>33; }
inline uint64_t digest(const void* p,size_t n,uint64_t h)
{
    auto* b=static_cast<const unsigned char*>(p);
    h=mix(h,n);
    for(;n>=8;n-=8,b+=8){uint64_t w;std::memcpy(&w,b,8);h=mix(h,w);}
    if(n){uint64_t w=0;std::memcpy(&w,b,n);h=mix(h,w);}
    return h;
}
constexpr uint32_t every=64; // a whole digest each 64 blocks
// Samples with every NaN as one pattern, 0x7FC00000: the NaN arithmetic makes from infinities is the CPU's default (x86's
// 0xFFC00000, arm64's 0x7FC00000), not the core's choice; that a sample is NaN is what the vectors hold.
inline const void* canonical(const void* p,size_t n,std::vector<uint32_t>& scratch)
{
    scratch.resize(n/4);std::memcpy(scratch.data(),p,n);
    for(auto& x:scratch)if((x&0x7F800000u)==0x7F800000u && (x&0x007FFFFFu))x=0x7FC00000u;
    return scratch.data();
}

class Tape
{
public:
    enum Mode { LiveRecord, LiveCheck, Record, Replay };
    Mode mode=LiveRecord;

    // Record: to `path` on save(), `header` its text. Replay: from `path`; plans and blocks are what the file holds.
    Tape(Mode m,std::string path,std::string header=""):mode(m),path_(std::move(path)),header_(std::move(header))
    {
        if(mode!=Replay)return;
        FILE* f=std::fopen(path_.c_str(),"rb");
        if(!f){std::printf("FAIL: no vectors at %s\n",path_.c_str());std::exit(1);}
        std::string line;int c;
        while((c=std::fgetc(f))!=EOF)
        {
            if(c!='\n'){line+=char(c);continue;}
            if(line=="binary")break;
            std::sscanf(line.c_str(),"plans: %u",&plans);std::sscanf(line.c_str(),"blocks: %u",&blocks);
            line.clear();
        }
        while((c=std::fgetc(f))!=EOF)data_.push_back(uint8_t(c));
        std::fclose(f);
        if(!plans || !blocks || data_.size()!=size_t(plans)*planBytes()){std::printf("FAIL: %s: %zu bytes, not %u plans of %u blocks\n",path_.c_str(),data_.size(),plans,blocks);std::exit(1);}
    }
    uint32_t plans=0,blocks=0;

    void plan(uint32_t p,uint32_t count)
    {
        if(mode==Replay && (p>=plans || count>blocks)){std::printf("FAIL: %s holds %u plans of %u blocks, not plan %u of %u\n",path_.c_str(),plans,blocks,p,count);std::exit(1);}
        if(mode==Record)
        {
            if(!plans)blocks=count;
            if(count!=blocks || p!=plans){std::printf("FAIL: plans are recorded in order, %u blocks each\n",blocks);std::exit(1);}
            data_.resize(size_t(++plans)*planBytes());
        }
        plan_=p;block_=0;running_=finish(p+1);at_=0;
        if(mode==LiveRecord)live_.clear();
    }
    // One value compared: `what` and `step` name it in a live mismatch.
    void see(const char* what,int step,const void* p,size_t n)
    {
        running_=digest(p,n,running_);
        if(mode==LiveRecord)live_.push_back({digest(p,n,0),what,block_,step});
        else if(mode==LiveCheck)
        {
            if(at_>=live_.size() || live_[at_].hash!=digest(p,n,0))
            {
                const Value& v=at_<live_.size()?live_[at_]:Value{0,what,block_,step};
                std::printf("MISMATCH rust_core_ against cpp_core_: %s, plan %u, block %u, step %d\n",v.what,plan_,v.block,v.step);std::exit(1);
            }
            ++at_;
        }
    }
    template<class T> void see(const char* what,int step,const T& x) { see(what,step,&x,sizeof x); }
    // The block's end.
    void block()
    {
        const uint64_t d=finish(running_);
        if(mode==Record || mode==Replay)
        {
            uint8_t* base=data_.data()+size_t(plan_)*planBytes();
            const uint16_t top=uint16_t(d>>48);
            if(mode==Record)std::memcpy(base+2*block_,&top,2);
            else
            {
                uint16_t want;std::memcpy(&want,base+2*block_,2);
                if(want!=top)mismatch("its 16-bit digest");
            }
            if((block_+1)%every==0 || block_+1==blocks)
            {
                uint8_t* at=base+2*size_t(blocks)+8*size_t(block_/every);
                if(mode==Record)std::memcpy(at,&d,8);
                else{uint64_t want;std::memcpy(&want,at,8);if(want!=d)mismatch("its 64-bit digest (the blocks' 16-bit ones all matched: the first difference is in the last 64 blocks)");}
            }
        }
        ++block_;
    }
    void endPlan() { if(mode==LiveCheck && at_!=live_.size()){std::printf("MISMATCH rust_core_ against cpp_core_: fewer values compared, plan %u\n",plan_);std::exit(1);} }
    void save() const
    {
        FILE* f=std::fopen(path_.c_str(),"wb");
        if(!f){std::printf("FAIL: cannot write %s\n",path_.c_str());std::exit(1);}
        std::fprintf(f,"%splans: %u\nblocks: %u\nevery: %u\nbinary\n",header_.c_str(),plans,blocks,every);
        std::fwrite(data_.data(),1,data_.size(),f);std::fclose(f);
    }

private:
    struct Value { uint64_t hash; const char* what; uint32_t block; int step; };
    size_t planBytes() const { return 2*size_t(blocks)+8*size_t((blocks+every-1)/every); }
    [[noreturn]] void mismatch(const char* how) const
    {
        std::printf("MISMATCH against the recorded C++ reference (%s): plan %u, block %u differs first, by %s\n",path_.c_str(),plan_,block_,how);std::exit(1);
    }
    std::string path_,header_;
    std::vector<uint8_t> data_;
    std::vector<Value> live_;
    uint32_t plan_=0,block_=0;size_t at_=0;
    uint64_t running_=0;
};

// The floating-point conditions record and replay share, for a file's header.
inline std::string conditions()
{
    return std::string("conditions: the Rust core cargo --release (always); the test and vectors_math.c -ffp-contract=off; exp, log1p and pow vectors_math.c's,\n"
                       "  not the platform libm's, in the reference and the Rust core alike; recorded in a ")+OSCAIRN_VECTORS_BUILD+" build, "+__VERSION__+
#if defined(__aarch64__)
           ", arm64"
#elif defined(__x86_64__)
           ", x86_64"
#endif
#if defined(__APPLE__)
           " macOS"
#elif defined(__linux__)
           " Linux"
#endif
           "\ndigest: vectors.h (a running 64-bit digest of every value compared; each block's top 16 bits, the whole each 64 blocks and at a plan's last)\n";
}

// A test's command line: [plans] [blocks] live; record FILE COMMIT [plans] [blocks]; replay FILE [plans] [blocks].
struct Args
{
    bool recording,replaying;
    uint32_t plans,blocks;
    Tape tape;
    Args(int argc,char** argv,uint32_t defaultPlans,uint32_t defaultBlocks,const std::string& test,const std::string& generator,const std::string& reference)
        :recording(argc>1 && !std::strcmp(argv[1],"record")),replaying(argc>1 && !std::strcmp(argv[1],"replay")),
         plans(size(argc,argv,0,defaultPlans)),blocks(size(argc,argv,1,defaultBlocks)),
         tape(recording?Tape::Record:replaying?Tape::Replay:Tape::LiveRecord,(recording || replaying) && argc>2?argv[2]:"",
              recording?"oscairn engine core vectors: "+test+"\ngenerator: "+generator+", plan n from seed n, n from 0\nreference: "+reference+" at "+(argc>3?argv[3]:"")+"\n"+conditions():"")
    {
        if((recording && argc<4) || (replaying && argc<3)){std::printf("usage: %s [record FILE COMMIT | replay FILE] [plans] [blocks]\n",argv[0]);std::exit(2);}
    }
    bool live() const { return !recording && !replaying; }
    // At the end: the file written; the cores' exp, log1p and pow (`math`: those the test reaches, a bit each) vectors_math.c's.
    const char* done(unsigned math)
    {
        if(recording)tape.save();
        for(unsigned i=0;i<3;++i)if(math>>i & 1 && !oscairn_vectors_math_calls[i])
        {
            std::printf("FAIL: the cores' %s was never vectors_math.c's: the platform's libm took it, whose rounding the vectors do not hold\n",(const char*[]){"exp","log1p","pow"}[i]);std::exit(1);
        }
        return recording?"cpp_core_ recorded":replaying?"rust_core_ as recorded":"rust_core_ and cpp_core_ identical";
    }
private:
    static uint32_t size(int argc,char** argv,int which,uint32_t otherwise)
    {
        const int at=(argc>1 && !std::strcmp(argv[1],"record")?4:argc>1 && !std::strcmp(argv[1],"replay")?3:1)+which;
        return argc>at?uint32_t(std::atoi(argv[at])):otherwise;
    }
};
}
