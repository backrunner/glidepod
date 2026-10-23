// SPDX-License-Identifier: Apache-2.0
// Original targeted Vulkan tests and indexed 3D workload. This is not a CTS.
#include "shaders.h"
#include <algorithm>
#include <android/native_window_jni.h>
#include <android/trace.h>
#include <array>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstring>
#include <functional>
#include <jni.h>
#include <mutex>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>
#include <vulkan/vulkan.h>

using Clock = std::chrono::steady_clock;
struct TraceScope {
  bool enabled;
  TraceScope(bool active, const char *name) : enabled(active) {
    if (enabled)
      ATrace_beginSection(name);
  }
  ~TraceScope() {
    if (enabled)
      ATrace_endSection();
  }
};
template <class F>
static auto traced(bool active, const char *name, F f) -> decltype(f()) {
  TraceScope scope(active, name);
  return f();
}
static int64_t ns(Clock::time_point t) {
  return std::chrono::duration_cast<std::chrono::nanoseconds>(
             t.time_since_epoch())
      .count();
}
static double ms(Clock::duration d) {
  return std::chrono::duration<double, std::milli>(d).count();
}
static void require(bool b, const std::string &message) {
  if (!b)
    throw std::runtime_error(message);
}
static void check(VkResult r, const char *name) {
  if (r != VK_SUCCESS)
    throw std::runtime_error(std::string(name) + ": VkResult " +
                             std::to_string(r));
}
#define VK(call) check((call), #call)
static std::string quoted(const std::string &s) {
  std::string out = "\"";
  for (unsigned char c : s) {
    if (c == '"' || c == '\\') {
      out += '\\';
      out += c;
    } else if (c < 32) {
      char escaped[7];
      snprintf(escaped, sizeof(escaped), "\\u%04x", c);
      out += escaped;
    } else
      out += c;
  }
  return out + '"';
}
template <typename T> static std::string numbers(const std::vector<T> &v) {
  std::ostringstream o;
  o.precision(10);
  o << '[';
  for (size_t i = 0; i < v.size(); i++) {
    if (i)
      o << ',';
    o << v[i];
  }
  o << ']';
  return o.str();
}
struct Json {
  std::vector<std::pair<std::string, std::string>> values;
  void raw(const std::string &k, const std::string &v) {
    values.emplace_back(k, v);
  }
  void str(const std::string &k, const std::string &v) { raw(k, quoted(v)); }
  template <typename T> void num(const std::string &k, T v) {
    raw(k, std::to_string(v));
  }
  void flag(const std::string &k, bool v) { raw(k, v ? "true" : "false"); }
  std::string dump() const {
    std::string r = "{";
    for (size_t i = 0; i < values.size(); i++) {
      if (i)
        r += ',';
      r += quoted(values[i].first) + ':' + values[i].second;
    }
    return r + '}';
  }
};
struct Job {
  std::atomic<bool> cancelled{false};
};
struct Buffer {
  VkBuffer handle{};
  VkDeviceMemory memory{};
  VkDeviceSize size{};
  bool coherent{};
};
struct Image {
  VkImage handle{};
  VkDeviceMemory memory{};
  VkImageView view{};
  VkFormat format{};
};
struct Target {
  VkImage image{};
  VkImageView view{};
  VkFramebuffer framebuffer{};
};
struct Vertex {
  float p[3], n[3], uv[2];
};

class Workload {
  Job &job;
  VkInstance instance{};
  VkSurfaceKHR surface{};
  VkPhysicalDevice physical{};
  VkDevice device{};
  VkQueue queue{};
  VkPhysicalDeviceProperties props{};
  VkPhysicalDeviceMemoryProperties memoryProps{};
  uint32_t family{}, timestampBits{};
  VkCommandPool commandPool{};
  VkCommandBuffer command{};
  VkFence fence{};
  VkSemaphore acquire{};
  VkQueryPool queries{};
  VkDescriptorPool descriptorPool{};
  VkDescriptorSetLayout textureLayout{}, storageLayout{};
  VkPipelineLayout graphicsLayout{}, computeLayout{};
  VkDescriptorSet textureSet{};
  std::vector<Buffer> buffers;
  std::vector<Image> images;
  std::vector<VkImageView> swapViews;
  std::vector<VkFramebuffer> framebuffers;
  std::vector<VkRenderPass> renderPasses;
  std::vector<VkPipeline> pipelines;
  std::vector<VkShaderModule> modules;
  std::vector<VkSemaphore> semaphores;
  std::vector<VkSwapchainKHR> swapchains;
  VkSampler sampler{};
  VkSwapchainKHR swapchain{};
  VkExtent2D extent{};
  VkFormat surfaceFormat{};
  VkColorSpaceKHR colorSpace{};
  std::vector<Target> targets;
  std::vector<VkSemaphore> presentDone;
  VkRenderPass screenPass{};
  VkPipeline screenPipeline{};
  Buffer vertices{}, indices{};
  uint32_t indexCount{};
  bool timelineSupported = false, dynamicSupported = false,
       sync2Supported = false, validationEnabled = false, negative = false;
  VkDebugUtilsMessengerEXT messenger{};
  std::mutex validationMutex;
  std::vector<std::string> validationMessages;
  uint32_t validationErrors = 0, validationWarnings = 0, recreations = 0,
           presented = 0;
  Json report;
  std::vector<std::string> checks;
  static constexpr uint64_t timeout = 5'000'000'000ULL;

  static VKAPI_ATTR VkBool32 VKAPI_CALL
  debug(VkDebugUtilsMessageSeverityFlagBitsEXT severity,
        VkDebugUtilsMessageTypeFlagsEXT,
        const VkDebugUtilsMessengerCallbackDataEXT *data, void *user) {
    auto &s = *static_cast<Workload *>(user);
    std::lock_guard<std::mutex> lock(s.validationMutex);
    if (severity & VK_DEBUG_UTILS_MESSAGE_SEVERITY_ERROR_BIT_EXT)
      s.validationErrors++;
    else if (severity & VK_DEBUG_UTILS_MESSAGE_SEVERITY_WARNING_BIT_EXT)
      s.validationWarnings++;
    if (s.validationMessages.size() < 20)
      s.validationMessages.emplace_back(data->pMessage);
    return VK_FALSE;
  }
  void cancelled() {
    require(!job.cancelled.load(),
            "Vulkan check cancelled: Android surface closed");
  }
  uint32_t memoryType(uint32_t bits, VkMemoryPropertyFlags required,
                      VkMemoryPropertyFlags preferred = 0) {
    for (int pass = 0; pass < 2; pass++)
      for (uint32_t i = 0; i < memoryProps.memoryTypeCount; i++) {
        auto flags = memoryProps.memoryTypes[i].propertyFlags;
        if ((bits & (1u << i)) && (flags & required) == required &&
            (pass || (flags & preferred) == preferred))
          return i;
      }
    throw std::runtime_error("No compatible Vulkan memory type");
  }
  Buffer buffer(VkDeviceSize size, VkBufferUsageFlags usage, bool host) {
    Buffer b{};
    b.size = size;
    VkBufferCreateInfo info{VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO};
    info.size = size;
    info.usage = usage;
    VK(vkCreateBuffer(device, &info, nullptr, &b.handle));
    VkMemoryRequirements req;
    vkGetBufferMemoryRequirements(device, b.handle, &req);
    uint32_t type = memoryType(req.memoryTypeBits,
                               host ? VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT : 0,
                               host ? VK_MEMORY_PROPERTY_HOST_COHERENT_BIT
                                    : VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT);
    b.coherent = (memoryProps.memoryTypes[type].propertyFlags &
                  VK_MEMORY_PROPERTY_HOST_COHERENT_BIT) != 0;
    VkMemoryAllocateInfo alloc{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO};
    alloc.allocationSize = req.size;
    alloc.memoryTypeIndex = type;
    VK(vkAllocateMemory(device, &alloc, nullptr, &b.memory));
    VK(vkBindBufferMemory(device, b.handle, b.memory, 0));
    buffers.push_back(b);
    return b;
  }
  void write(const Buffer &b, const void *data, size_t count) {
    require(count <= b.size, "Buffer write exceeds allocation");
    void *mapped;
    VK(vkMapMemory(device, b.memory, 0, VK_WHOLE_SIZE, 0, &mapped));
    memcpy(mapped, data, count);
    if (!b.coherent) {
      VkMappedMemoryRange range{VK_STRUCTURE_TYPE_MAPPED_MEMORY_RANGE};
      range.memory = b.memory;
      range.size = VK_WHOLE_SIZE;
      VK(vkFlushMappedMemoryRanges(device, 1, &range));
    }
    vkUnmapMemory(device, b.memory);
  }
  std::vector<uint8_t> read(const Buffer &b) {
    void *mapped;
    VK(vkMapMemory(device, b.memory, 0, VK_WHOLE_SIZE, 0, &mapped));
    if (!b.coherent) {
      VkMappedMemoryRange range{VK_STRUCTURE_TYPE_MAPPED_MEMORY_RANGE};
      range.memory = b.memory;
      range.size = VK_WHOLE_SIZE;
      VK(vkInvalidateMappedMemoryRanges(device, 1, &range));
    }
    std::vector<uint8_t> result(b.size);
    memcpy(result.data(), mapped, b.size);
    vkUnmapMemory(device, b.memory);
    return result;
  }
  VkImageView view(VkImage image, VkFormat format, VkImageAspectFlags aspect) {
    VkImageViewCreateInfo info{VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO};
    info.image = image;
    info.viewType = VK_IMAGE_VIEW_TYPE_2D;
    info.format = format;
    info.subresourceRange = {aspect, 0, 1, 0, 1};
    VkImageView v;
    VK(vkCreateImageView(device, &info, nullptr, &v));
    return v;
  }
  Image image(uint32_t w, uint32_t h, VkFormat format, VkImageUsageFlags usage,
              VkImageAspectFlags aspect = VK_IMAGE_ASPECT_COLOR_BIT) {
    Image im{};
    im.format = format;
    VkImageCreateInfo info{VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
    info.imageType = VK_IMAGE_TYPE_2D;
    info.format = format;
    info.extent = {w, h, 1};
    info.mipLevels = 1;
    info.arrayLayers = 1;
    info.samples = VK_SAMPLE_COUNT_1_BIT;
    info.tiling = VK_IMAGE_TILING_OPTIMAL;
    info.usage = usage;
    VK(vkCreateImage(device, &info, nullptr, &im.handle));
    VkMemoryRequirements req;
    vkGetImageMemoryRequirements(device, im.handle, &req);
    VkMemoryAllocateInfo alloc{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO};
    alloc.allocationSize = req.size;
    alloc.memoryTypeIndex =
        memoryType(req.memoryTypeBits, 0, VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT);
    VK(vkAllocateMemory(device, &alloc, nullptr, &im.memory));
    VK(vkBindImageMemory(device, im.handle, im.memory, 0));
    im.view = view(im.handle, format, aspect);
    images.push_back(im);
    return im;
  }
  void barrier(VkImage im, VkImageLayout old, VkImageLayout next,
               VkAccessFlags src, VkAccessFlags dst, VkPipelineStageFlags from,
               VkPipelineStageFlags to) {
    VkImageMemoryBarrier b{VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER};
    b.oldLayout = old;
    b.newLayout = next;
    b.srcAccessMask = src;
    b.dstAccessMask = dst;
    b.srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    b.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    b.image = im;
    b.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    vkCmdPipelineBarrier(command, from, to, 0, 0, nullptr, 0, nullptr, 1, &b);
  }
  void bufferBarrier(const Buffer &b, VkAccessFlags src, VkAccessFlags dst,
                     VkPipelineStageFlags from, VkPipelineStageFlags to) {
    VkBufferMemoryBarrier m{VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER};
    m.srcAccessMask = src;
    m.dstAccessMask = dst;
    m.srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    m.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    m.buffer = b.handle;
    m.size = VK_WHOLE_SIZE;
    vkCmdPipelineBarrier(command, from, to, 0, 0, nullptr, 1, &m, 0, nullptr);
  }
  void begin() {
    cancelled();
    VK(vkWaitForFences(device, 1, &fence, VK_TRUE, timeout));
    VK(vkResetCommandBuffer(command, 0));
    VkCommandBufferBeginInfo info{VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO};
    info.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    VK(vkBeginCommandBuffer(command, &info));
  }
  void submit(VkSemaphore wait = VK_NULL_HANDLE,
              VkSemaphore signal = VK_NULL_HANDLE) {
    VK(vkEndCommandBuffer(command));
    VK(vkResetFences(device, 1, &fence));
    VkSubmitInfo s{VK_STRUCTURE_TYPE_SUBMIT_INFO};
    s.commandBufferCount = 1;
    s.pCommandBuffers = &command;
    VkPipelineStageFlags stage = VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT;
    if (wait) {
      s.waitSemaphoreCount = 1;
      s.pWaitSemaphores = &wait;
      s.pWaitDstStageMask = &stage;
    }
    if (signal) {
      s.signalSemaphoreCount = 1;
      s.pSignalSemaphores = &signal;
    }
    VK(vkQueueSubmit(queue, 1, &s, fence));
  }
  void complete() { VK(vkWaitForFences(device, 1, &fence, VK_TRUE, timeout)); }
  VkSemaphore semaphore() {
    VkSemaphoreCreateInfo info{VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO};
    VkSemaphore s;
    VK(vkCreateSemaphore(device, &info, nullptr, &s));
    semaphores.push_back(s);
    return s;
  }
  void passed(const std::string &name) { checks.push_back(quoted(name)); }
  void initialize(ANativeWindow *window, bool validation) {
    uint32_t loaderVersion = VK_API_VERSION_1_0;
    auto enumerateVersion = reinterpret_cast<PFN_vkEnumerateInstanceVersion>(
        vkGetInstanceProcAddr(nullptr, "vkEnumerateInstanceVersion"));
    if (enumerateVersion)
      VK(enumerateVersion(&loaderVersion));
    uint32_t requested = std::min(loaderVersion, VK_API_VERSION_1_3);
    require(requested >= VK_API_VERSION_1_1,
            "Vulkan 1.1 is required by this check");
    VkApplicationInfo app{VK_STRUCTURE_TYPE_APPLICATION_INFO};
    app.pApplicationName = "GlidePod Vulkan Check";
    app.apiVersion = requested;
    const char *layer = "VK_LAYER_KHRONOS_validation";
    std::vector<const char *> extensions = {
        VK_KHR_SURFACE_EXTENSION_NAME, VK_KHR_ANDROID_SURFACE_EXTENSION_NAME};
    if (validation) {
      extensions.push_back(VK_EXT_DEBUG_UTILS_EXTENSION_NAME);
      extensions.push_back(VK_EXT_VALIDATION_FEATURES_EXTENSION_NAME);
    }
    VkDebugUtilsMessengerCreateInfoEXT debugInfo{
        VK_STRUCTURE_TYPE_DEBUG_UTILS_MESSENGER_CREATE_INFO_EXT};
    debugInfo.messageSeverity = VK_DEBUG_UTILS_MESSAGE_SEVERITY_ERROR_BIT_EXT |
                                VK_DEBUG_UTILS_MESSAGE_SEVERITY_WARNING_BIT_EXT;
    debugInfo.messageType = VK_DEBUG_UTILS_MESSAGE_TYPE_GENERAL_BIT_EXT |
                            VK_DEBUG_UTILS_MESSAGE_TYPE_VALIDATION_BIT_EXT |
                            VK_DEBUG_UTILS_MESSAGE_TYPE_PERFORMANCE_BIT_EXT;
    debugInfo.pfnUserCallback = debug;
    debugInfo.pUserData = this;
    VkValidationFeatureEnableEXT syncValidation =
        VK_VALIDATION_FEATURE_ENABLE_SYNCHRONIZATION_VALIDATION_EXT;
    VkValidationFeaturesEXT validationFeatures{
        VK_STRUCTURE_TYPE_VALIDATION_FEATURES_EXT};
    validationFeatures.enabledValidationFeatureCount = 1;
    validationFeatures.pEnabledValidationFeatures = &syncValidation;
    validationFeatures.pNext = &debugInfo;
    VkInstanceCreateInfo info{VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO};
    info.pApplicationInfo = &app;
    info.enabledExtensionCount = extensions.size();
    info.ppEnabledExtensionNames = extensions.data();
    if (validation) {
      info.enabledLayerCount = 1;
      info.ppEnabledLayerNames = &layer;
      info.pNext = &validationFeatures;
    }
    VK(vkCreateInstance(&info, nullptr, &instance));
    validationEnabled = validation;
    if (validation) {
      auto create = reinterpret_cast<PFN_vkCreateDebugUtilsMessengerEXT>(
          vkGetInstanceProcAddr(instance, "vkCreateDebugUtilsMessengerEXT"));
      require(create, "Debug utils unavailable");
      VK(create(instance, &debugInfo, nullptr, &messenger));
    }
    VkAndroidSurfaceCreateInfoKHR surfaceInfo{
        VK_STRUCTURE_TYPE_ANDROID_SURFACE_CREATE_INFO_KHR};
    surfaceInfo.window = window;
    VK(vkCreateAndroidSurfaceKHR(instance, &surfaceInfo, nullptr, &surface));
    uint32_t count = 0;
    VK(vkEnumeratePhysicalDevices(instance, &count, nullptr));
    require(count > 0, "No Vulkan physical device");
    std::vector<VkPhysicalDevice> devices(count);
    VK(vkEnumeratePhysicalDevices(instance, &count, devices.data()));
    for (auto candidate : devices) {
      uint32_t n = 0;
      vkGetPhysicalDeviceQueueFamilyProperties(candidate, &n, nullptr);
      std::vector<VkQueueFamilyProperties> families(n);
      vkGetPhysicalDeviceQueueFamilyProperties(candidate, &n, families.data());
      for (uint32_t i = 0; i < n; i++) {
        VkBool32 present;
        VK(vkGetPhysicalDeviceSurfaceSupportKHR(candidate, i, surface,
                                                &present));
        if (present && (families[i].queueFlags &
                        (VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT)) ==
                           (VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT)) {
          physical = candidate;
          family = i;
          timestampBits = families[i].timestampValidBits;
          break;
        }
      }
      if (physical)
        break;
    }
    require(physical,
            "No queue supports graphics, compute and Android presentation");
    vkGetPhysicalDeviceProperties(physical, &props);
    vkGetPhysicalDeviceMemoryProperties(physical, &memoryProps);
    VK(vkEnumerateDeviceExtensionProperties(physical, nullptr, &count,
                                            nullptr));
    std::vector<VkExtensionProperties> available(count);
    VK(vkEnumerateDeviceExtensionProperties(physical, nullptr, &count,
                                            available.data()));
    std::string extensionJson = "[";
    for (size_t i = 0; i < available.size(); i++) {
      if (i)
        extensionJson += ',';
      extensionJson += quoted(available[i].extensionName);
    }
    extensionJson += ']';
    report.raw("device_extensions", extensionJson);
    VkPhysicalDeviceTimelineSemaphoreFeatures timeline{
        VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_TIMELINE_SEMAPHORE_FEATURES};
    VkPhysicalDeviceVulkan13Features v13{
        VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES};
    VkPhysicalDeviceFeatures2 features{
        VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2};
    if (requested >= VK_API_VERSION_1_2 &&
        props.apiVersion >= VK_API_VERSION_1_2)
      features.pNext = &timeline;
    if (requested >= VK_API_VERSION_1_3 &&
        props.apiVersion >= VK_API_VERSION_1_3)
      timeline.pNext = &v13;
    vkGetPhysicalDeviceFeatures2(physical, &features);
    Json advertised;
    advertised.flag("geometry_shader", features.features.geometryShader);
    advertised.flag("sampler_anisotropy", features.features.samplerAnisotropy);
    advertised.flag("texture_compression_etc2",
                    features.features.textureCompressionETC2);
    advertised.flag("texture_compression_astc_ldr",
                    features.features.textureCompressionASTC_LDR);
    advertised.flag("texture_compression_bc",
                    features.features.textureCompressionBC);
    advertised.flag("multi_draw_indirect", features.features.multiDrawIndirect);
    report.raw("advertised_features_not_all_exercised", advertised.dump());
    Json formats;
    for (auto format :
         {VK_FORMAT_R8G8B8A8_UNORM, VK_FORMAT_B8G8R8A8_UNORM,
          VK_FORMAT_R16G16B16A16_SFLOAT, VK_FORMAT_D32_SFLOAT,
          VK_FORMAT_ETC2_R8G8B8A8_UNORM_BLOCK, VK_FORMAT_ASTC_4x4_UNORM_BLOCK,
          VK_FORMAT_BC3_UNORM_BLOCK}) {
      VkFormatProperties properties;
      vkGetPhysicalDeviceFormatProperties(physical, format, &properties);
      formats.num(std::to_string(format), properties.optimalTilingFeatures);
    }
    report.raw("advertised_optimal_format_features", formats.dump());
    timelineSupported = timeline.timelineSemaphore;
    dynamicSupported = v13.dynamicRendering;
    sync2Supported = v13.synchronization2;
    // Query all capabilities, but enable only the two Vulkan 1.3 features used
    // here.
    v13 = VkPhysicalDeviceVulkan13Features{
        VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES};
    v13.dynamicRendering = dynamicSupported;
    v13.synchronization2 = sync2Supported;
    float priority = 1;
    VkDeviceQueueCreateInfo qi{VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO};
    qi.queueFamilyIndex = family;
    qi.queueCount = 1;
    qi.pQueuePriorities = &priority;
    const char *swap = VK_KHR_SWAPCHAIN_EXTENSION_NAME;
    VkDeviceCreateInfo di{VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO};
    di.queueCreateInfoCount = 1;
    di.pQueueCreateInfos = &qi;
    di.enabledExtensionCount = 1;
    di.ppEnabledExtensionNames = &swap;
    if (features.pNext)
      di.pNext = &timeline;
    VK(vkCreateDevice(physical, &di, nullptr, &device));
    vkGetDeviceQueue(device, family, 0, &queue);
    VkCommandPoolCreateInfo pi{VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO};
    pi.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT;
    pi.queueFamilyIndex = family;
    VK(vkCreateCommandPool(device, &pi, nullptr, &commandPool));
    VkCommandBufferAllocateInfo ai{
        VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO};
    ai.commandPool = commandPool;
    ai.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
    ai.commandBufferCount = 1;
    VK(vkAllocateCommandBuffers(device, &ai, &command));
    VkFenceCreateInfo fi{VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};
    fi.flags = VK_FENCE_CREATE_SIGNALED_BIT;
    VK(vkCreateFence(device, &fi, nullptr, &fence));
    acquire = semaphore();
    if (timestampBits) {
      VkQueryPoolCreateInfo q{VK_STRUCTURE_TYPE_QUERY_POOL_CREATE_INFO};
      q.queryType = VK_QUERY_TYPE_TIMESTAMP;
      q.queryCount = 2;
      VK(vkCreateQueryPool(device, &q, nullptr, &queries));
    }
    VkDescriptorPoolSize sizes[] = {
        {VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER, 4},
        {VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, 4}};
    VkDescriptorPoolCreateInfo dp{
        VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO};
    dp.maxSets = 8;
    dp.poolSizeCount = 2;
    dp.pPoolSizes = sizes;
    VK(vkCreateDescriptorPool(device, &dp, nullptr, &descriptorPool));
    auto layout = [&](VkDescriptorType type, VkShaderStageFlags stage,
                      VkDescriptorSetLayout &out) {
      VkDescriptorSetLayoutBinding binding{0, type, 1, stage, nullptr};
      VkDescriptorSetLayoutCreateInfo li{
          VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO};
      li.bindingCount = 1;
      li.pBindings = &binding;
      VK(vkCreateDescriptorSetLayout(device, &li, nullptr, &out));
    };
    layout(VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
           VK_SHADER_STAGE_FRAGMENT_BIT, textureLayout);
    layout(VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, VK_SHADER_STAGE_COMPUTE_BIT,
           storageLayout);
    VkPushConstantRange push{VK_SHADER_STAGE_VERTEX_BIT, 0, 32};
    VkPipelineLayoutCreateInfo pl{
        VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO};
    pl.setLayoutCount = 1;
    pl.pSetLayouts = &textureLayout;
    pl.pushConstantRangeCount = 1;
    pl.pPushConstantRanges = &push;
    VK(vkCreatePipelineLayout(device, &pl, nullptr, &graphicsLayout));
    pl.pSetLayouts = &storageLayout;
    pl.pushConstantRangeCount = 0;
    VK(vkCreatePipelineLayout(device, &pl, nullptr, &computeLayout));
    report.str("api", "vulkan");
    report.str("device_name", props.deviceName);
    report.num("api_version", props.apiVersion);
    report.num("requested_api_version", requested);
    report.num("driver_version", props.driverVersion);
    report.num("vendor_id", props.vendorID);
    report.num("device_type", props.deviceType);
    report.num("max_image_dimension_2d", props.limits.maxImageDimension2D);
    report.num("max_compute_work_group_invocations",
               props.limits.maxComputeWorkGroupInvocations);
    report.num("timestamp_valid_bits", timestampBits);
    report.num("timestamp_period_ns", props.limits.timestampPeriod);
    report.flag("timeline_semaphore_supported", timelineSupported);
    report.flag("dynamic_rendering_supported", dynamicSupported);
    report.flag("synchronization2_supported", sync2Supported);
    passed("instance-device-graphics-compute-present-queue");
  }
  VkDescriptorSet allocateSet(VkDescriptorSetLayout layout) {
    VkDescriptorSetAllocateInfo ai{
        VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO};
    ai.descriptorPool = descriptorPool;
    ai.descriptorSetCount = 1;
    ai.pSetLayouts = &layout;
    VkDescriptorSet s;
    VK(vkAllocateDescriptorSets(device, &ai, &s));
    return s;
  }
  template <size_t N> VkShaderModule shader(const uint32_t (&code)[N]) {
    VkShaderModuleCreateInfo s{VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO};
    s.codeSize = sizeof(code);
    s.pCode = code;
    VkShaderModule m;
    VK(vkCreateShaderModule(device, &s, nullptr, &m));
    modules.push_back(m);
    return m;
  }
  void computeCheck() {
    std::array<uint32_t, 256> input{};
    for (size_t i = 0; i < input.size(); i++)
      input[i] = i;
    Buffer upload =
        buffer(sizeof(input), VK_BUFFER_USAGE_TRANSFER_SRC_BIT, true);
    write(upload, input.data(), sizeof(input));
    Buffer storage = buffer(sizeof(input),
                            VK_BUFFER_USAGE_STORAGE_BUFFER_BIT |
                                VK_BUFFER_USAGE_TRANSFER_DST_BIT |
                                VK_BUFFER_USAGE_TRANSFER_SRC_BIT,
                            false);
    Buffer download =
        buffer(sizeof(input), VK_BUFFER_USAGE_TRANSFER_DST_BIT, true);
    auto set = allocateSet(storageLayout);
    VkDescriptorBufferInfo bi{storage.handle, 0, storage.size};
    VkWriteDescriptorSet w{VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET};
    w.dstSet = set;
    w.descriptorCount = 1;
    w.descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER;
    w.pBufferInfo = &bi;
    vkUpdateDescriptorSets(device, 1, &w, 0, nullptr);
    VkComputePipelineCreateInfo ci{
        VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO};
    ci.layout = computeLayout;
    ci.stage = {VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                nullptr,
                0,
                VK_SHADER_STAGE_COMPUTE_BIT,
                shader(check_comp),
                "main",
                nullptr};
    VkPipeline pipe;
    VK(vkCreateComputePipelines(device, VK_NULL_HANDLE, 1, &ci, nullptr,
                                &pipe));
    pipelines.push_back(pipe);
    begin();
    VkBufferCopy copy{0, 0, sizeof(input)};
    vkCmdCopyBuffer(command, upload.handle, storage.handle, 1, &copy);
    bufferBarrier(storage, VK_ACCESS_TRANSFER_WRITE_BIT,
                  VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT,
                  VK_PIPELINE_STAGE_TRANSFER_BIT,
                  VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT);
    vkCmdBindPipeline(command, VK_PIPELINE_BIND_POINT_COMPUTE, pipe);
    vkCmdBindDescriptorSets(command, VK_PIPELINE_BIND_POINT_COMPUTE,
                            computeLayout, 0, 1, &set, 0, nullptr);
    vkCmdDispatch(command, 4, 1, 1);
    bufferBarrier(storage, VK_ACCESS_SHADER_WRITE_BIT,
                  VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT,
                  VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT,
                  VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT);
    vkCmdDispatch(command, 4, 1, 1);
    bufferBarrier(
        storage, VK_ACCESS_SHADER_WRITE_BIT, VK_ACCESS_TRANSFER_READ_BIT,
        VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT);
    vkCmdCopyBuffer(command, storage.handle, download.handle, 1, &copy);
    bufferBarrier(download, VK_ACCESS_TRANSFER_WRITE_BIT,
                  VK_ACCESS_HOST_READ_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT,
                  VK_PIPELINE_STAGE_HOST_BIT);
    submit();
    complete();
    auto data = read(download);
    std::array<uint32_t, 256> output;
    memcpy(output.data(), data.data(), data.size());
    for (size_t i = 0; i < output.size(); i++)
      require(output[i] == (i * 3 + 7) * 3 + 7,
              "Compute/storage barrier/readback mismatch at " +
                  std::to_string(i));
    passed("staging-copy-compute-storage-two-dispatch-barrier-readback");
    if (sync2Supported) {
      auto sync = reinterpret_cast<PFN_vkCmdPipelineBarrier2>(
          vkGetDeviceProcAddr(device, "vkCmdPipelineBarrier2"));
      require(sync, "Missing synchronization2 entry point");
      begin();
      vkCmdFillBuffer(command, storage.handle, 0, VK_WHOLE_SIZE, 0x1a2b3c4d);
      VkBufferMemoryBarrier2 b{VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER_2};
      b.srcStageMask = VK_PIPELINE_STAGE_2_TRANSFER_BIT;
      b.srcAccessMask = VK_ACCESS_2_TRANSFER_WRITE_BIT;
      b.dstStageMask = VK_PIPELINE_STAGE_2_TRANSFER_BIT;
      b.dstAccessMask = VK_ACCESS_2_TRANSFER_READ_BIT;
      b.srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
      b.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
      b.buffer = storage.handle;
      b.size = VK_WHOLE_SIZE;
      VkDependencyInfo dep{VK_STRUCTURE_TYPE_DEPENDENCY_INFO};
      dep.bufferMemoryBarrierCount = 1;
      dep.pBufferMemoryBarriers = &b;
      sync(command, &dep);
      vkCmdCopyBuffer(command, storage.handle, download.handle, 1, &copy);
      bufferBarrier(download, VK_ACCESS_TRANSFER_WRITE_BIT,
                    VK_ACCESS_HOST_READ_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT,
                    VK_PIPELINE_STAGE_HOST_BIT);
      submit();
      complete();
      data = read(download);
      memcpy(output.data(), data.data(), data.size());
      for (auto value : output)
        require(value == 0x1a2b3c4d,
                "Synchronization2 transfer readback mismatch");
      passed("synchronization2-fill-copy-host-readback");
    }
    if (timelineSupported) {
      VkSemaphoreTypeCreateInfo type{
          VK_STRUCTURE_TYPE_SEMAPHORE_TYPE_CREATE_INFO};
      type.semaphoreType = VK_SEMAPHORE_TYPE_TIMELINE;
      VkSemaphoreCreateInfo si{VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO};
      si.pNext = &type;
      VkSemaphore sem;
      VK(vkCreateSemaphore(device, &si, nullptr, &sem));
      semaphores.push_back(sem);
      uint64_t value = 7;
      VkTimelineSemaphoreSubmitInfo ts{
          VK_STRUCTURE_TYPE_TIMELINE_SEMAPHORE_SUBMIT_INFO};
      ts.signalSemaphoreValueCount = 1;
      ts.pSignalSemaphoreValues = &value;
      VkSubmitInfo submitInfo{VK_STRUCTURE_TYPE_SUBMIT_INFO};
      submitInfo.pNext = &ts;
      submitInfo.signalSemaphoreCount = 1;
      submitInfo.pSignalSemaphores = &sem;
      VK(vkQueueSubmit(queue, 1, &submitInfo, VK_NULL_HANDLE));
      auto wait = reinterpret_cast<PFN_vkWaitSemaphores>(
          vkGetDeviceProcAddr(device, "vkWaitSemaphores"));
      auto counter = reinterpret_cast<PFN_vkGetSemaphoreCounterValue>(
          vkGetDeviceProcAddr(device, "vkGetSemaphoreCounterValue"));
      require(wait && counter, "Reported timeline support has no entry points");
      VkSemaphoreWaitInfo wi{VK_STRUCTURE_TYPE_SEMAPHORE_WAIT_INFO};
      wi.semaphoreCount = 1;
      wi.pSemaphores = &sem;
      wi.pValues = &value;
      VK(wait(device, &wi, timeout));
      uint64_t current = 0;
      VK(counter(device, sem, &current));
      require(current == 7, "Timeline semaphore counter mismatch");
      passed("timeline-semaphore-submit-wait-counter");
    }
  }
  void texture() {
    const std::array<uint8_t, 16> pixels = {
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255};
    Buffer upload =
        buffer(pixels.size(), VK_BUFFER_USAGE_TRANSFER_SRC_BIT, true);
    write(upload, pixels.data(), pixels.size());
    Image tex =
        image(2, 2, VK_FORMAT_R8G8B8A8_UNORM,
              VK_IMAGE_USAGE_TRANSFER_DST_BIT | VK_IMAGE_USAGE_SAMPLED_BIT);
    begin();
    barrier(tex.handle, VK_IMAGE_LAYOUT_UNDEFINED,
            VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 0,
            VK_ACCESS_TRANSFER_WRITE_BIT, VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,
            VK_PIPELINE_STAGE_TRANSFER_BIT);
    VkBufferImageCopy copy{};
    copy.imageSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    copy.imageExtent = {2, 2, 1};
    vkCmdCopyBufferToImage(command, upload.handle, tex.handle,
                           VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &copy);
    barrier(tex.handle, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
            VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
            VK_ACCESS_TRANSFER_WRITE_BIT, VK_ACCESS_SHADER_READ_BIT,
            VK_PIPELINE_STAGE_TRANSFER_BIT,
            VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT);
    submit();
    complete();
    VkSamplerCreateInfo si{VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO};
    si.magFilter = VK_FILTER_NEAREST;
    si.minFilter = VK_FILTER_NEAREST;
    si.mipmapMode = VK_SAMPLER_MIPMAP_MODE_NEAREST;
    si.addressModeU = si.addressModeV = si.addressModeW =
        VK_SAMPLER_ADDRESS_MODE_REPEAT;
    VK(vkCreateSampler(device, &si, nullptr, &sampler));
    textureSet = allocateSet(textureLayout);
    VkDescriptorImageInfo di{sampler, tex.view,
                             VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL};
    VkWriteDescriptorSet w{VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET};
    w.dstSet = textureSet;
    w.descriptorCount = 1;
    w.descriptorType = VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER;
    w.pImageInfo = &di;
    vkUpdateDescriptorSets(device, 1, &w, 0, nullptr);
  }
  VkRenderPass renderPass(VkFormat format) {
    VkAttachmentDescription attachments[2]{};
    attachments[0].format = format;
    attachments[0].samples = VK_SAMPLE_COUNT_1_BIT;
    attachments[0].loadOp = VK_ATTACHMENT_LOAD_OP_CLEAR;
    attachments[0].storeOp = VK_ATTACHMENT_STORE_OP_STORE;
    attachments[0].stencilLoadOp = VK_ATTACHMENT_LOAD_OP_DONT_CARE;
    attachments[0].stencilStoreOp = VK_ATTACHMENT_STORE_OP_DONT_CARE;
    attachments[0].initialLayout = VK_IMAGE_LAYOUT_UNDEFINED;
    attachments[0].finalLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL;
    attachments[1] = attachments[0];
    attachments[1].format = VK_FORMAT_D32_SFLOAT;
    attachments[1].storeOp = VK_ATTACHMENT_STORE_OP_DONT_CARE;
    attachments[1].finalLayout =
        VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL;
    VkAttachmentReference color{0, VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL},
        depth{1, VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL};
    VkSubpassDescription sub{};
    sub.pipelineBindPoint = VK_PIPELINE_BIND_POINT_GRAPHICS;
    sub.colorAttachmentCount = 1;
    sub.pColorAttachments = &color;
    sub.pDepthStencilAttachment = &depth;
    VkSubpassDependency dep{VK_SUBPASS_EXTERNAL,
                            0,
                            VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT |
                                VK_PIPELINE_STAGE_EARLY_FRAGMENT_TESTS_BIT,
                            VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT |
                                VK_PIPELINE_STAGE_EARLY_FRAGMENT_TESTS_BIT,
                            0,
                            VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT |
                                VK_ACCESS_DEPTH_STENCIL_ATTACHMENT_WRITE_BIT,
                            0};
    VkRenderPassCreateInfo info{VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO};
    info.attachmentCount = 2;
    info.pAttachments = attachments;
    info.subpassCount = 1;
    info.pSubpasses = &sub;
    info.dependencyCount = 1;
    info.pDependencies = &dep;
    VkRenderPass p;
    VK(vkCreateRenderPass(device, &info, nullptr, &p));
    renderPasses.push_back(p);
    return p;
  }
  VkFramebuffer framebuffer(VkRenderPass pass, VkImageView color, uint32_t w,
                            uint32_t h) {
    Image depth = image(w, h, VK_FORMAT_D32_SFLOAT,
                        VK_IMAGE_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT,
                        VK_IMAGE_ASPECT_DEPTH_BIT);
    VkImageView views[] = {color, depth.view};
    VkFramebufferCreateInfo info{VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO};
    info.renderPass = pass;
    info.attachmentCount = 2;
    info.pAttachments = views;
    info.width = w;
    info.height = h;
    info.layers = 1;
    VkFramebuffer f;
    VK(vkCreateFramebuffer(device, &info, nullptr, &f));
    framebuffers.push_back(f);
    return f;
  }
  VkPipeline pipeline(VkRenderPass pass, bool scene, bool depthTest = false,
                      bool dynamicRendering = false) {
    VkPipelineShaderStageCreateInfo stages[2] = {
        {VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO, nullptr, 0,
         VK_SHADER_STAGE_VERTEX_BIT,
         scene ? shader(scene_vert)
               : (depthTest ? shader(depth_vert) : shader(check_vert)),
         "main", nullptr},
        {VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO, nullptr, 0,
         VK_SHADER_STAGE_FRAGMENT_BIT,
         scene ? shader(scene_frag)
               : (depthTest ? shader(depth_frag) : shader(check_frag)),
         "main", nullptr}};
    VkVertexInputBindingDescription binding{0, sizeof(Vertex),
                                            VK_VERTEX_INPUT_RATE_VERTEX};
    VkVertexInputAttributeDescription attrs[] = {
        {0, 0, VK_FORMAT_R32G32B32_SFLOAT, 0},
        {1, 0, VK_FORMAT_R32G32B32_SFLOAT, 12},
        {2, 0, VK_FORMAT_R32G32_SFLOAT, 24}};
    VkPipelineVertexInputStateCreateInfo vertex{
        VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO};
    if (scene) {
      vertex.vertexBindingDescriptionCount = 1;
      vertex.pVertexBindingDescriptions = &binding;
      vertex.vertexAttributeDescriptionCount = 3;
      vertex.pVertexAttributeDescriptions = attrs;
    }
    VkPipelineInputAssemblyStateCreateInfo assembly{
        VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO};
    assembly.topology = VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST;
    VkPipelineViewportStateCreateInfo viewport{
        VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO};
    viewport.viewportCount = 1;
    viewport.scissorCount = 1;
    VkPipelineRasterizationStateCreateInfo raster{
        VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO};
    raster.polygonMode = VK_POLYGON_MODE_FILL;
    raster.cullMode = VK_CULL_MODE_NONE;
    raster.frontFace = VK_FRONT_FACE_COUNTER_CLOCKWISE;
    raster.lineWidth = 1;
    VkPipelineMultisampleStateCreateInfo multi{
        VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO};
    multi.rasterizationSamples = VK_SAMPLE_COUNT_1_BIT;
    VkPipelineDepthStencilStateCreateInfo depth{
        VK_STRUCTURE_TYPE_PIPELINE_DEPTH_STENCIL_STATE_CREATE_INFO};
    depth.depthTestEnable = scene || depthTest;
    depth.depthWriteEnable = scene || depthTest;
    depth.depthCompareOp = VK_COMPARE_OP_LESS;
    VkPipelineColorBlendAttachmentState blend{};
    blend.colorWriteMask = 0xf;
    VkPipelineColorBlendStateCreateInfo color{
        VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO};
    color.attachmentCount = 1;
    color.pAttachments = &blend;
    VkDynamicState states[] = {VK_DYNAMIC_STATE_VIEWPORT,
                               VK_DYNAMIC_STATE_SCISSOR};
    VkPipelineDynamicStateCreateInfo dynamic{
        VK_STRUCTURE_TYPE_PIPELINE_DYNAMIC_STATE_CREATE_INFO};
    dynamic.dynamicStateCount = 2;
    dynamic.pDynamicStates = states;
    VkGraphicsPipelineCreateInfo info{
        VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO};
    info.stageCount = 2;
    info.pStages = stages;
    info.pVertexInputState = &vertex;
    info.pInputAssemblyState = &assembly;
    info.pViewportState = &viewport;
    info.pRasterizationState = &raster;
    info.pMultisampleState = &multi;
    info.pDepthStencilState = &depth;
    info.pColorBlendState = &color;
    info.pDynamicState = &dynamic;
    info.layout = graphicsLayout;
    info.renderPass = pass;
    VkFormat format = VK_FORMAT_R8G8B8A8_UNORM;
    VkPipelineRenderingCreateInfo rendering{
        VK_STRUCTURE_TYPE_PIPELINE_RENDERING_CREATE_INFO};
    rendering.colorAttachmentCount = 1;
    rendering.pColorAttachmentFormats = &format;
    if (dynamicRendering)
      info.pNext = &rendering;
    VkPipeline p;
    VK(vkCreateGraphicsPipelines(device, VK_NULL_HANDLE, 1, &info, nullptr,
                                 &p));
    pipelines.push_back(p);
    return p;
  }
  void render(VkRenderPass pass, VkFramebuffer fb, VkPipeline pipe, uint32_t w,
              uint32_t h, int objects, int frame, bool scene,
              bool depthTest = false, bool marker = false) {
    VkClearValue clear[2]{};
    clear[0].color = {{.055f, .075f, .065f, 1}};
    clear[1].depthStencil = {1, 0};
    VkRenderPassBeginInfo info{VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO};
    info.renderPass = pass;
    info.framebuffer = fb;
    info.renderArea.extent = {w, h};
    info.clearValueCount = 2;
    info.pClearValues = clear;
    vkCmdBeginRenderPass(command, &info, VK_SUBPASS_CONTENTS_INLINE);
    VkViewport viewport{0, 0, float(w), float(h), 0, 1};
    VkRect2D scissor{{0, 0}, {w, h}};
    vkCmdSetViewport(command, 0, 1, &viewport);
    vkCmdSetScissor(command, 0, 1, &scissor);
    vkCmdBindPipeline(command, VK_PIPELINE_BIND_POINT_GRAPHICS, pipe);
    vkCmdBindDescriptorSets(command, VK_PIPELINE_BIND_POINT_GRAPHICS,
                            graphicsLayout, 0, 1, &textureSet, 0, nullptr);
    if (scene) {
      VkDeviceSize offset = 0;
      vkCmdBindVertexBuffers(command, 0, 1, &vertices.handle, &offset);
      vkCmdBindIndexBuffer(command, indices.handle, 0, VK_INDEX_TYPE_UINT16);
      int grid = int(std::ceil(std::sqrt(objects)));
      float step = 2.5f / grid, scale = 1.15f / grid;
      for (int i = 0; i < objects; i++) {
        float params[] = {float((frame * .8 + i * 13) * .01745329252),
                          ((i % grid) - (grid - 1) * .5f) * step,
                          ((i / grid) - (grid - 1) * .5f) * step,
                          scale,
                          float(w) / h,
                          0,
                          0,
                          0};
        vkCmdPushConstants(command, graphicsLayout, VK_SHADER_STAGE_VERTEX_BIT,
                           0, sizeof(params), params);
        vkCmdDrawIndexed(command, indexCount, 1, 0, 0, 0);
      }
    } else if (depthTest) {
      const float params[][8] = {{1, 0, 0, 1, .2f, 0, 0, 0},
                                 {0, 1, 0, 1, .8f, 0, 0, 0},
                                 {0, 0, 1, 1, .5f, 0, 0, 0}};
      for (auto &p : params) {
        vkCmdPushConstants(command, graphicsLayout, VK_SHADER_STAGE_VERTEX_BIT,
                           0, sizeof(p), p);
        vkCmdDraw(command, 3, 1, 0, 0);
      }
    } else
      vkCmdDraw(command, 3, 1, 0, 0);
    if (marker) {
      // Diagnostic only: identify actual new content at the host scanout.
      // A sequence counter alone also counts repeated copies of the same image.
      VkClearAttachment stamp{};
      stamp.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
      stamp.colorAttachment = 0;
      stamp.clearValue.color = {{float(frame & 255) / 255.f,
                                 float((frame >> 8) & 255) / 255.f,
                                 167.f / 255.f, 1}};
      VkClearRect rect{{{0, 0}, {8, 8}}, 0, 1};
      vkCmdClearAttachments(command, 1, &stamp, 1, &rect);
    }
    vkCmdEndRenderPass(command);
  }
  void mesh() {
    std::vector<Vertex> vs;
    std::vector<uint16_t> ix;
    for (int i = 0; i <= 32; i++)
      for (int j = 0; j <= 16; j++) {
        float u = i * 6.28318530718f / 32, v = j * 6.28318530718f / 16,
              x = std::cos(u), y = std::sin(u), c = std::cos(v),
              s = std::sin(v);
        vs.push_back({{x * (.65f + .25f * c), y * (.65f + .25f * c), .25f * s},
                      {x * c, y * c, s},
                      {i / 8.f, j / 4.f}});
      }
    for (int i = 0; i < 32; i++)
      for (int j = 0; j < 16; j++) {
        uint16_t a = i * 17 + j, b = (i + 1) * 17 + j;
        ix.insert(ix.end(),
                  {a, b, uint16_t(b + 1), a, uint16_t(b + 1), uint16_t(a + 1)});
      }
    vertices = buffer(vs.size() * sizeof(Vertex),
                      VK_BUFFER_USAGE_VERTEX_BUFFER_BIT, true);
    write(vertices, vs.data(), vertices.size);
    indices = buffer(ix.size() * sizeof(uint16_t),
                     VK_BUFFER_USAGE_INDEX_BUFFER_BIT, true);
    write(indices, ix.data(), indices.size);
    indexCount = ix.size();
  }
  std::vector<uint8_t> offscreen(VkRenderPass pass, VkPipeline pipe, bool scene,
                                 bool depthTest = false) {
    constexpr uint32_t size = 64;
    Image target = image(size, size, VK_FORMAT_R8G8B8A8_UNORM,
                         VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT |
                             VK_IMAGE_USAGE_TRANSFER_SRC_BIT);
    auto fb = framebuffer(pass, target.view, size, size);
    Buffer output =
        buffer(size * size * 4, VK_BUFFER_USAGE_TRANSFER_DST_BIT, true);
    begin();
    render(pass, fb, pipe, size, size, 1, 0, scene, depthTest);
    barrier(target.handle, VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
            VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
            VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT, VK_ACCESS_TRANSFER_READ_BIT,
            VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
            VK_PIPELINE_STAGE_TRANSFER_BIT);
    VkBufferImageCopy copy{};
    copy.imageSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    copy.imageExtent = {size, size, 1};
    vkCmdCopyImageToBuffer(command, target.handle,
                           VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, output.handle,
                           1, &copy);
    bufferBarrier(output, VK_ACCESS_TRANSFER_WRITE_BIT, VK_ACCESS_HOST_READ_BIT,
                  VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_HOST_BIT);
    submit();
    complete();
    return read(output);
  }
  void quadrants(const std::vector<uint8_t> &pixels) {
    const int positions[][2] = {{16, 16}, {48, 16}, {16, 48}, {48, 48}};
    const int colors[][4] = {{255, 0, 0, 255},
                             {0, 255, 0, 255},
                             {0, 0, 255, 255},
                             {255, 255, 255, 255}};
    for (int q = 0; q < 4; q++)
      for (int c = 0; c < 4; c++) {
        int actual = pixels[(positions[q][1] * 64 + positions[q][0]) * 4 + c];
        require(std::abs(actual - colors[q][c]) <= 2,
                "Texture pixel mismatch: quadrant " + std::to_string(q) +
                    " channel " + std::to_string(c) + " actual " +
                    std::to_string(actual));
      }
  }
  void pixelChecks() {
    auto pass = renderPass(VK_FORMAT_R8G8B8A8_UNORM);
    auto pixels = offscreen(pass, pipeline(pass, false), false);
    if (negative)
      pixels[(16 * 64 + 16) * 4] ^= 255;
    quadrants(pixels);
    passed("texture-upload-descriptor-sampling-four-quadrant-pixels");
    auto depth = offscreen(pass, pipeline(pass, false, true), false, true);
    for (int y : {16, 48})
      for (int x : {16, 48})
        for (int c = 0; c < 4; c++)
          require(std::abs(int(depth[(y * 64 + x) * 4 + c]) -
                           ((c == 0 || c == 3) ? 255 : 0)) <= 2,
                  "Depth occlusion pixel mismatch");
    passed("depth-less-near-occludes-later-far-triangles");
    auto scene = offscreen(pass, pipeline(pass, true), true);
    int colored = 0;
    for (size_t i = 0; i < scene.size(); i += 4)
      if (scene[i] > 40 || scene[i + 1] > 40 || scene[i + 2] > 40)
        colored++;
    require(colored > 100, "Indexed 3D scene produced an empty framebuffer");
    report.num("colored_pixels", colored);
    passed("indexed-vertex-push-constant-depth-scene-readback");
    if (dynamicSupported) {
      auto beginRendering = reinterpret_cast<PFN_vkCmdBeginRendering>(
          vkGetDeviceProcAddr(device, "vkCmdBeginRendering"));
      auto endRendering = reinterpret_cast<PFN_vkCmdEndRendering>(
          vkGetDeviceProcAddr(device, "vkCmdEndRendering"));
      require(beginRendering && endRendering,
              "Missing dynamic rendering entry points");
      auto pipe = pipeline(VK_NULL_HANDLE, false, false, true);
      auto target = image(64, 64, VK_FORMAT_R8G8B8A8_UNORM,
                          VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT |
                              VK_IMAGE_USAGE_TRANSFER_SRC_BIT);
      auto output = buffer(64 * 64 * 4, VK_BUFFER_USAGE_TRANSFER_DST_BIT, true);
      begin();
      barrier(target.handle, VK_IMAGE_LAYOUT_UNDEFINED,
              VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL, 0,
              VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
              VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,
              VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT);
      VkRenderingAttachmentInfo attachment{
          VK_STRUCTURE_TYPE_RENDERING_ATTACHMENT_INFO};
      attachment.imageView = target.view;
      attachment.imageLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL;
      attachment.loadOp = VK_ATTACHMENT_LOAD_OP_CLEAR;
      attachment.storeOp = VK_ATTACHMENT_STORE_OP_STORE;
      VkRenderingInfo rendering{VK_STRUCTURE_TYPE_RENDERING_INFO};
      rendering.renderArea.extent = {64, 64};
      rendering.layerCount = 1;
      rendering.colorAttachmentCount = 1;
      rendering.pColorAttachments = &attachment;
      beginRendering(command, &rendering);
      VkViewport vp{0, 0, 64, 64, 0, 1};
      VkRect2D scissor{{0, 0}, {64, 64}};
      vkCmdSetViewport(command, 0, 1, &vp);
      vkCmdSetScissor(command, 0, 1, &scissor);
      vkCmdBindPipeline(command, VK_PIPELINE_BIND_POINT_GRAPHICS, pipe);
      vkCmdBindDescriptorSets(command, VK_PIPELINE_BIND_POINT_GRAPHICS,
                              graphicsLayout, 0, 1, &textureSet, 0, nullptr);
      vkCmdDraw(command, 3, 1, 0, 0);
      endRendering(command);
      barrier(target.handle, VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
              VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
              VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT, VK_ACCESS_TRANSFER_READ_BIT,
              VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
              VK_PIPELINE_STAGE_TRANSFER_BIT);
      VkBufferImageCopy copy{};
      copy.imageSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
      copy.imageExtent = {64, 64, 1};
      vkCmdCopyImageToBuffer(command, target.handle,
                             VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                             output.handle, 1, &copy);
      bufferBarrier(output, VK_ACCESS_TRANSFER_WRITE_BIT,
                    VK_ACCESS_HOST_READ_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT,
                    VK_PIPELINE_STAGE_HOST_BIT);
      submit();
      complete();
      quadrants(read(output));
      passed("dynamic-rendering-textured-pixel-readback");
    }
  }
  void swap() {
    VkSurfaceCapabilitiesKHR caps;
    VK(vkGetPhysicalDeviceSurfaceCapabilitiesKHR(physical, surface, &caps));
    require(caps.supportedUsageFlags & VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT,
            "Surface does not support color attachments");
    if (!swapchain) {
      uint32_t count;
      VK(vkGetPhysicalDeviceSurfaceFormatsKHR(physical, surface, &count,
                                              nullptr));
      require(count > 0, "No Android surface formats");
      std::vector<VkSurfaceFormatKHR> formats(count);
      VK(vkGetPhysicalDeviceSurfaceFormatsKHR(physical, surface, &count,
                                              formats.data()));
      auto choice = std::find_if(formats.begin(), formats.end(), [](auto &f) {
        return f.format == VK_FORMAT_R8G8B8A8_UNORM ||
               f.format == VK_FORMAT_B8G8R8A8_UNORM;
      });
      require(choice != formats.end(),
              "No RGBA8/BGRA8 UNORM Android surface format");
      surfaceFormat = choice->format;
      colorSpace = choice->colorSpace;
      screenPass = renderPass(surfaceFormat);
      screenPipeline = pipeline(screenPass, true);
    }
    extent = caps.currentExtent;
    require(extent.width != UINT32_MAX && extent.height != UINT32_MAX,
            "Android surface extent unavailable");
    uint32_t count = caps.minImageCount + 1;
    if (caps.maxImageCount)
      count = std::min(count, caps.maxImageCount);
    VkSwapchainCreateInfoKHR info{VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR};
    info.surface = surface;
    info.minImageCount = count;
    info.imageFormat = surfaceFormat;
    info.imageColorSpace = colorSpace;
    info.imageExtent = extent;
    info.imageArrayLayers = 1;
    info.imageUsage = VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT;
    info.preTransform = caps.currentTransform;
    info.compositeAlpha = VK_COMPOSITE_ALPHA_INHERIT_BIT_KHR;
    if (!(caps.supportedCompositeAlpha & info.compositeAlpha))
      info.compositeAlpha = VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR;
    require(caps.supportedCompositeAlpha & info.compositeAlpha,
            "No supported surface alpha mode");
    info.presentMode = VK_PRESENT_MODE_FIFO_KHR;
    info.clipped = VK_TRUE;
    info.oldSwapchain = swapchain;
    VkSwapchainKHR next;
    VK(vkCreateSwapchainKHR(device, &info, nullptr, &next));
    swapchain = next;
    swapchains.push_back(next);
    VK(vkGetSwapchainImagesKHR(device, swapchain, &count, nullptr));
    std::vector<VkImage> ims(count);
    VK(vkGetSwapchainImagesKHR(device, swapchain, &count, ims.data()));
    targets.clear();
    presentDone.clear();
    for (auto im : ims) {
      auto v = view(im, surfaceFormat, VK_IMAGE_ASPECT_COLOR_BIT);
      swapViews.push_back(v);
      targets.push_back(
          {im, v, framebuffer(screenPass, v, extent.width, extent.height)});
      presentDone.push_back(semaphore());
    }
  }
  void frames(int objects, int samples, bool recreate, bool stages) {
    swap();
    uint32_t originalWidth = extent.width, originalHeight = extent.height;
    report.num("width", extent.width);
    report.num("height", extent.height);
    report.num("surface_format", surfaceFormat);
    report.str("present_mode", "FIFO");
    std::vector<double> draws, intervals, gpu, recreationMs;
    std::vector<std::string> stageRecords;
    auto first = Clock::now(), previous = first;
    bool previousMeasured = false;
    uint32_t frame = 0;
    bool recreated = false;
    uint32_t generationFrame = 0;
    while (draws.size() < size_t(samples)) {
      cancelled();
      require(ms(Clock::now() - first) < 150000,
              "Vulkan frame loop exceeded 150 seconds");
      if (recreate && !recreated && draws.size() >= size_t(samples / 2)) {
        auto recreateStart = Clock::now();
        VK(vkDeviceWaitIdle(device));
        traced(stages, "GlidePod.vk.recreate", [&] { swap(); });
        recreationMs.push_back(ms(Clock::now() - recreateStart));
        recreations++;
        recreated = true;
        generationFrame = 0;
        require(extent.width == originalWidth &&
                    extent.height == originalHeight,
                "Unexpected Android surface size change during measurement");
      }
      auto loopStart = Clock::now();
      traced(stages, "GlidePod.vk.begin", [&] { begin(); });
      auto beginDone = Clock::now();
      uint32_t index;
      VkResult acquired = traced(stages, "GlidePod.vk.acquire", [&] {
        return vkAcquireNextImageKHR(device, swapchain, timeout, acquire,
                                     VK_NULL_HANDLE, &index);
      });
      if (acquired == VK_ERROR_OUT_OF_DATE_KHR) {
        VK(vkEndCommandBuffer(command));
        VK(vkDeviceWaitIdle(device));
        swap();
        recreations++;
        generationFrame = 0;
        continue;
      }
      require(acquired == VK_SUCCESS || acquired == VK_SUBOPTIMAL_KHR,
              "Swapchain acquire failed: " + std::to_string(acquired));
      auto start = Clock::now();
      bool measured = ms(start - first) >= 2000;
      if (stages) {
        ATrace_setCounter("GlidePod.swapchain_generation", swapchains.size());
        ATrace_setCounter("GlidePod.swapchain_frame", generationFrame);
      }
      {
        TraceScope trace(stages, "GlidePod.vk.draw-submit");
        if (queries) {
          vkCmdResetQueryPool(command, queries, 0, 2);
          vkCmdWriteTimestamp(command, VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,
                              queries, 0);
        }
        render(screenPass, targets[index].framebuffer, screenPipeline,
               extent.width, extent.height, objects, frame++, true, false,
               stages);
        if (queries)
          vkCmdWriteTimestamp(command, VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT,
                              queries, 1);
        barrier(targets[index].image, VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
                VK_IMAGE_LAYOUT_PRESENT_SRC_KHR,
                VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT, 0,
                VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
                VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT);
        submit(acquire, presentDone[index]);
      }
      auto submitted = Clock::now();
      VkPresentInfoKHR present{VK_STRUCTURE_TYPE_PRESENT_INFO_KHR};
      present.waitSemaphoreCount = 1;
      present.pWaitSemaphores = &presentDone[index];
      present.swapchainCount = 1;
      present.pSwapchains = &swapchain;
      present.pImageIndices = &index;
      VkResult result = traced(stages, "GlidePod.vk.present", [&] {
        return vkQueuePresentKHR(queue, &present);
      });
      auto presentDoneAt = Clock::now();
      require(result == VK_SUCCESS || result == VK_SUBOPTIMAL_KHR ||
                  result == VK_ERROR_OUT_OF_DATE_KHR,
              "Queue present failed: " + std::to_string(result));
      if (result == VK_ERROR_OUT_OF_DATE_KHR) {
        complete();
        VK(vkDeviceWaitIdle(device));
        swap();
        recreations++;
        generationFrame = 0;
        continue;
      }
      presented++;
      traced(stages, "GlidePod.vk.complete", [&] { complete(); });
      auto completeAt = Clock::now();
      int sampleIndex = (measured && previousMeasured) ? int(draws.size()) : -1;
      if (measured && previousMeasured) {
        draws.push_back(ms(submitted - start));
        intervals.push_back(ms(start - previous));
        if (queries) {
          uint64_t stamps[2];
          traced(stages, "GlidePod.vk.query", [&] {
            VK(vkGetQueryPoolResults(
                device, queries, 0, 2, sizeof(stamps), stamps, sizeof(uint64_t),
                VK_QUERY_RESULT_64_BIT | VK_QUERY_RESULT_WAIT_BIT));
          });
          uint64_t mask = timestampBits == 64
                              ? UINT64_MAX
                              : ((uint64_t(1) << timestampBits) - 1);
          gpu.push_back(double((stamps[1] - stamps[0]) & mask) *
                        props.limits.timestampPeriod / 1e6);
        }
      }
      auto queryDoneAt = Clock::now();
      if (stages && stageRecords.size() < 128 &&
          (generationFrame < 12 ||
           (sampleIndex >= samples / 2 - 8 &&
            sampleIndex <= samples / 2 + 12) ||
           ms(queryDoneAt - loopStart) > 30 ||
           (sampleIndex >= 0 && ms(start - previous) > 30))) {
        Json row;
        row.num("frame", frame - 1);
        row.num("sample", sampleIndex);
        row.num("generation", swapchains.size());
        row.num("generation_frame", generationFrame);
        row.num("image", index);
        row.num("loop_start_monotonic_ns", ns(loopStart));
        row.num("acquired_monotonic_ns", ns(start));
        row.num("begin_ms", ms(beginDone - loopStart));
        row.num("acquire_ms", ms(start - beginDone));
        row.num("draw_submit_ms", ms(submitted - start));
        row.num("present_ms", ms(presentDoneAt - submitted));
        row.num("complete_ms", ms(completeAt - presentDoneAt));
        row.num("query_ms", ms(queryDoneAt - completeAt));
        row.num("loop_ms", ms(queryDoneAt - loopStart));
        row.num("interval_ms", ms(start - previous));
        stageRecords.push_back(row.dump());
      }
      generationFrame++;
      previous = start;
      previousMeasured = measured;
    }
    VK(vkDeviceWaitIdle(device));
    report.num("objects", objects);
    report.num("triangles_per_frame", objects * 1024);
    report.num("warmup_seconds", 2);
    report.raw("draw_ms", numbers(draws));
    report.raw("frame_interval_ms", numbers(intervals));
    report.str("frame_interval_scope", "successful-image-acquisition");
    report.flag("frame_interval_includes_recreation", true);
    report.raw("device_timestamp_ms", numbers(gpu));
    report.str("timestamp_scope",
               "Reported device timestamps around the render pass; host "
               "translation accuracy is not calibrated");
    report.num("swapchain_recreations", recreations);
    report.flag("recreate_requested", recreate);
    report.flag("stage_timings_enabled", stages);
    std::string stageJson = "[";
    for (size_t i = 0; i < stageRecords.size(); i++) {
      if (i)
        stageJson += ',';
      stageJson += stageRecords[i];
    }
    stageJson += ']';
    report.raw("stage_timings", stageJson);
    report.raw("explicit_swapchain_recreation_ms", numbers(recreationMs));
    report.num("presented_frames", presented);
    report.str("scheduling",
               "Intervals between successful image acquisitions; "
               "includes FIFO queue backpressure, fence completion "
               "and swapchain lifecycle. Not display timing");
    passed("android-swapchain-acquire-submit-present");
    if (recreate)
      passed("same-extent-swapchain-recreation");
  }
  void cleanup() noexcept {
    if (device) {
      vkDeviceWaitIdle(device);
      for (auto f : framebuffers)
        vkDestroyFramebuffer(device, f, nullptr);
      for (auto p : pipelines)
        vkDestroyPipeline(device, p, nullptr);
      for (auto p : renderPasses)
        vkDestroyRenderPass(device, p, nullptr);
      if (graphicsLayout)
        vkDestroyPipelineLayout(device, graphicsLayout, nullptr);
      if (computeLayout)
        vkDestroyPipelineLayout(device, computeLayout, nullptr);
      if (descriptorPool)
        vkDestroyDescriptorPool(device, descriptorPool, nullptr);
      if (textureLayout)
        vkDestroyDescriptorSetLayout(device, textureLayout, nullptr);
      if (storageLayout)
        vkDestroyDescriptorSetLayout(device, storageLayout, nullptr);
      if (sampler)
        vkDestroySampler(device, sampler, nullptr);
      for (auto m : modules)
        vkDestroyShaderModule(device, m, nullptr);
      for (auto im : images) {
        vkDestroyImageView(device, im.view, nullptr);
        vkDestroyImage(device, im.handle, nullptr);
        vkFreeMemory(device, im.memory, nullptr);
      }
      for (auto b : buffers) {
        vkDestroyBuffer(device, b.handle, nullptr);
        vkFreeMemory(device, b.memory, nullptr);
      }
      for (auto v : swapViews)
        vkDestroyImageView(device, v, nullptr);
      if (fence)
        vkDestroyFence(device, fence, nullptr);
      for (auto s : semaphores)
        vkDestroySemaphore(device, s, nullptr);
      if (queries)
        vkDestroyQueryPool(device, queries, nullptr);
      if (commandPool)
        vkDestroyCommandPool(device, commandPool, nullptr);
      for (auto s : swapchains)
        vkDestroySwapchainKHR(device, s, nullptr);
      vkDestroyDevice(device, nullptr);
      device = VK_NULL_HANDLE;
    }
    if (surface) {
      vkDestroySurfaceKHR(instance, surface, nullptr);
      surface = VK_NULL_HANDLE;
    }
    if (messenger) {
      auto destroy = reinterpret_cast<PFN_vkDestroyDebugUtilsMessengerEXT>(
          vkGetInstanceProcAddr(instance, "vkDestroyDebugUtilsMessengerEXT"));
      if (destroy)
        destroy(instance, messenger, nullptr);
      messenger = VK_NULL_HANDLE;
    }
    if (instance) {
      vkDestroyInstance(instance, nullptr);
      instance = VK_NULL_HANDLE;
    }
  }

public:
  explicit Workload(Job &j) : job(j) {}
  ~Workload() { cleanup(); }
  std::string run(ANativeWindow *window, int objects, int samples,
                  bool validation, bool inject, bool recreate, bool stages) {
    negative = inject;
    try {
      initialize(window, validation);
      computeCheck();
      texture();
      mesh();
      pixelChecks();
      frames(objects, samples, recreate, stages);
    } catch (const std::exception &e) {
      report.str("error", e.what());
    }
    cleanup(); // Include teardown diagnostics before publishing the result.
    std::string list = "[";
    for (size_t i = 0; i < checks.size(); i++) {
      if (i)
        list += ',';
      list += checks[i];
    }
    list += ']';
    report.raw("checks", list);
    report.flag("validation_enabled", validationEnabled);
    report.flag("synchronization_validation", validationEnabled);
    report.num("validation_errors", validationErrors);
    report.num("validation_warnings", validationWarnings);
    std::string messages = "[";
    for (size_t i = 0; i < validationMessages.size(); i++) {
      if (i)
        messages += ',';
      messages += quoted(validationMessages[i]);
    }
    messages += ']';
    report.raw("validation_messages", messages);
    report.flag("negative_control", negative);
    report.flag(
        "vk_success",
        presented > 0 && validationErrors == 0 &&
            std::none_of(report.values.begin(), report.values.end(),
                         [](auto &pair) { return pair.first == "error"; }));
    return report.dump();
  }
};
extern "C" JNIEXPORT jlong JNICALL
Java_dev_glidepod_vulkan_MainActivity_createJob(JNIEnv *, jclass) {
  return reinterpret_cast<jlong>(new Job());
}
extern "C" JNIEXPORT void JNICALL
Java_dev_glidepod_vulkan_MainActivity_cancelJob(JNIEnv *, jclass, jlong id) {
  reinterpret_cast<Job *>(id)->cancelled = true;
}
extern "C" JNIEXPORT void JNICALL
Java_dev_glidepod_vulkan_MainActivity_destroyJob(JNIEnv *, jclass, jlong id) {
  delete reinterpret_cast<Job *>(id);
}
extern "C" JNIEXPORT jstring JNICALL
Java_dev_glidepod_vulkan_MainActivity_execute(JNIEnv *env, jclass, jlong id,
                                              jobject surface, jint objects,
                                              jint samples, jboolean validation,
                                              jboolean negative,
                                              jboolean recreate,
                                              jboolean stages) {
  ANativeWindow *window = ANativeWindow_fromSurface(env, surface);
  if (!window)
    return env->NewStringUTF("{\"error\":\"Android surface is unavailable\"}");
  Workload w(*reinterpret_cast<Job *>(id));
  std::string result =
      w.run(window, objects, samples, validation, negative, recreate, stages);
  ANativeWindow_release(window);
  return env->NewStringUTF(result.c_str());
}
