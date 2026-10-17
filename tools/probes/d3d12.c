#define COBJMACROS
#include "probe.h"

#include <d3d12.h>
#include <dxgi1_4.h>

#define BUFFERS 2

static void transition(ID3D12GraphicsCommandList *list, ID3D12Resource *resource, D3D12_RESOURCE_STATES before,
                       D3D12_RESOURCE_STATES after)
{
    D3D12_RESOURCE_BARRIER barrier = {0};

    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition.pResource = resource;
    barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barrier.Transition.StateBefore = before;
    barrier.Transition.StateAfter = after;
    ID3D12GraphicsCommandList_ResourceBarrier(list, 1, &barrier);
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show)
{
    HWND window;
    IDXGIFactory4 *factory = NULL;
    ID3D12Device *device = NULL;
    ID3D12CommandQueue *queue = NULL;
    IDXGISwapChain1 *chain1 = NULL;
    IDXGISwapChain3 *chain = NULL;
    ID3D12DescriptorHeap *heap = NULL;
    ID3D12Resource *buffers[BUFFERS] = {0};
    ID3D12CommandAllocator *allocator = NULL;
    ID3D12GraphicsCommandList *list = NULL;
    ID3D12Fence *fence = NULL;
    HANDLE event;
    UINT64 fence_value = 0;
    D3D12_COMMAND_QUEUE_DESC queue_desc = {0};
    DXGI_SWAP_CHAIN_DESC1 chain_desc = {0};
    D3D12_DESCRIPTOR_HEAP_DESC heap_desc = {0};
    D3D12_CPU_DESCRIPTOR_HANDLE first;
    UINT stride;
    HRESULT hr;
    unsigned int frame = 0;
    int i;

    (void)instance;
    (void)previous;
    (void)command_line;
    (void)show;
    probe_open_log("d3d12");
    window = probe_window("Carafe D3D12 probe", probe_default_proc);
    hr = CreateDXGIFactory1(&IID_IDXGIFactory4, (void **)&factory);
    if (FAILED(hr))
    {
        probe_log("CreateDXGIFactory1 failed: 0x%08lx", (unsigned long)hr);
        return 3;
    }
    hr = D3D12CreateDevice(NULL, D3D_FEATURE_LEVEL_11_0, &IID_ID3D12Device, (void **)&device);
    if (FAILED(hr))
    {
        probe_log("D3D12CreateDevice failed: 0x%08lx", (unsigned long)hr);
        return 4;
    }
    queue_desc.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    ID3D12Device_CreateCommandQueue(device, &queue_desc, &IID_ID3D12CommandQueue, (void **)&queue);
    chain_desc.Width = PROBE_WIDTH;
    chain_desc.Height = PROBE_HEIGHT;
    chain_desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    chain_desc.SampleDesc.Count = 1;
    chain_desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    chain_desc.BufferCount = BUFFERS;
    chain_desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    hr = IDXGIFactory4_CreateSwapChainForHwnd(factory, (IUnknown *)queue, window, &chain_desc, NULL, NULL, &chain1);
    if (FAILED(hr))
    {
        probe_log("CreateSwapChainForHwnd failed: 0x%08lx", (unsigned long)hr);
        return 5;
    }
    IDXGISwapChain1_QueryInterface(chain1, &IID_IDXGISwapChain3, (void **)&chain);
    heap_desc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    heap_desc.NumDescriptors = BUFFERS;
    ID3D12Device_CreateDescriptorHeap(device, &heap_desc, &IID_ID3D12DescriptorHeap, (void **)&heap);
    stride = ID3D12Device_GetDescriptorHandleIncrementSize(device, D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
    heap->lpVtbl->GetCPUDescriptorHandleForHeapStart(heap, &first);
    for (i = 0; i < BUFFERS; i++)
    {
        D3D12_CPU_DESCRIPTOR_HANDLE handle = {first.ptr + (SIZE_T)i * stride};
        IDXGISwapChain3_GetBuffer(chain, (UINT)i, &IID_ID3D12Resource, (void **)&buffers[i]);
        ID3D12Device_CreateRenderTargetView(device, buffers[i], NULL, handle);
    }
    ID3D12Device_CreateCommandAllocator(device, D3D12_COMMAND_LIST_TYPE_DIRECT, &IID_ID3D12CommandAllocator,
                                        (void **)&allocator);
    ID3D12Device_CreateCommandList(device, 0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator, NULL,
                                   &IID_ID3D12GraphicsCommandList, (void **)&list);
    ID3D12GraphicsCommandList_Close(list);
    ID3D12Device_CreateFence(device, 0, D3D12_FENCE_FLAG_NONE, &IID_ID3D12Fence, (void **)&fence);
    event = CreateEventA(NULL, FALSE, FALSE, NULL);
    probe_log("device ready");
    while (probe_pump())
    {
        UINT index = IDXGISwapChain3_GetCurrentBackBufferIndex(chain);
        D3D12_CPU_DESCRIPTOR_HANDLE handle = {first.ptr + (SIZE_T)index * stride};
        ID3D12CommandList *lists[1];

        ID3D12CommandAllocator_Reset(allocator);
        ID3D12GraphicsCommandList_Reset(list, allocator, NULL);
        transition(list, buffers[index], D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_RENDER_TARGET);
        for (i = 0; i < 4; i++)
        {
            const struct probe_quadrant *q = &probe_quadrants[i];
            D3D12_RECT rect = {q->left, q->top, q->right, q->bottom};
            float color[4] = {q->r, q->g, q->b, 1.0f};
            ID3D12GraphicsCommandList_ClearRenderTargetView(list, handle, color, 1, &rect);
        }
        transition(list, buffers[index], D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PRESENT);
        ID3D12GraphicsCommandList_Close(list);
        lists[0] = (ID3D12CommandList *)list;
        ID3D12CommandQueue_ExecuteCommandLists(queue, 1, lists);
        hr = IDXGISwapChain3_Present(chain, 1, 0);
        if (FAILED(hr))
        {
            probe_log("Present failed: 0x%08lx", (unsigned long)hr);
            break;
        }
        fence_value++;
        ID3D12CommandQueue_Signal(queue, fence, fence_value);
        if (ID3D12Fence_GetCompletedValue(fence) < fence_value)
        {
            ID3D12Fence_SetEventOnCompletion(fence, fence_value, event);
            WaitForSingleObject(event, 5000);
        }
        probe_frame_done(++frame);
    }
    probe_log("exit");
    return 0;
}
