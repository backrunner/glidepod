#include "stress.hpp"
#include "topology.hpp"
#include <jni.h>
extern "C" JNIEXPORT jstring JNICALL Java_dev_glidepod_benchmark_Stress_nativeRun(
    JNIEnv* env, jclass, jstring name, jint width, jint height, jint seconds) {
    const char* chars=env->GetStringUTFChars(name,nullptr);
    if(!chars)return nullptr;
    std::string mode(chars);env->ReleaseStringUTFChars(name,chars);
    try { return env->NewStringUTF(glidepod_stress::run(mode,width,height,seconds).c_str()); }
    catch(const std::exception& e) { env->ThrowNew(env->FindClass("java/lang/IllegalStateException"),e.what()); return nullptr; }
}
extern "C" JNIEXPORT jstring JNICALL Java_dev_glidepod_benchmark_Stress_nativeTopology(JNIEnv* env, jclass) {
    try { return env->NewStringUTF(glidepod_topology::run().json.c_str()); }
    catch(const std::exception& e) { env->ThrowNew(env->FindClass("java/lang/IllegalStateException"),e.what()); return nullptr; }
}
