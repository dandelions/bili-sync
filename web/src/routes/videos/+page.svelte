<script lang="ts">
	import VideoCard from '$lib/components/video-card.svelte';
	import Pagination from '$lib/components/pagination.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import * as AlertDialog from '$lib/components/ui/alert-dialog/index.js';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import SquarePenIcon from '@lucide/svelte/icons/square-pen';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import Grid2X2Icon from '@lucide/svelte/icons/grid-2x2';
	import ListIcon from '@lucide/svelte/icons/list';
	import InfoIcon from '@lucide/svelte/icons/info';
	import api from '$lib/api';
	import { Checkbox } from '$lib/components/ui/checkbox/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import type {
		VideosResponse,
		VideoSourcesResponse,
		ApiError,
		VideoSource,
		UpdateFilteredVideoStatusRequest,
		VideoInfo
	} from '$lib/types';
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import { VIDEO_SOURCES } from '$lib/consts';
	import { setBreadcrumb } from '$lib/stores/breadcrumb';
	import {
		appStateStore,
		resetCurrentPage,
		setAll,
		setCurrentPage,
		setCreatedTimeFilter,
		setQuery,
		setStatusFilter,
		setValidationFilter,
		ToQuery,
		ToFilterParams,
		hasActiveFilters,
		type StatusFilterValue,
		type ValidationFilterValue
	} from '$lib/stores/filter';
	import { toast } from 'svelte-sonner';
	import DropdownFilter, { type Filter } from '$lib/components/dropdown-filter.svelte';
	import SearchBar from '$lib/components/search-bar.svelte';
	import FilteredStatusEditor from '$lib/components/filtered-status-editor.svelte';
	import StatusFilter from '$lib/components/status-filter.svelte';
	import ValidationFilter from '$lib/components/validation-filter.svelte';
	import CreatedTimeFilter from '$lib/components/created-time-filter.svelte';
	import { SvelteMap } from 'svelte/reactivity';

	const pageSize = 20;

	let videosData: VideosResponse | null = null;
	let loading = false;
	let viewMode: 'card' | 'list' = 'card';
	let selectedVideoIds = new Set<number>();
	let deleteDialogOpen = false;
	let deleting = false;

	let lastSearch: string | null = null;

	let resetAllDialogOpen = false;
	let resettingAll = false;

	let forceReset = false;

	let updateAllDialogOpen = false;
	let updatingAll = false;

	let videoSources: VideoSourcesResponse | null = null;
	let videoSourcesLoaded = false;
	let filters: Record<string, Filter> | null = null;
	let sourceMap: SvelteMap<string, { type: string; name: string }> = new SvelteMap();

	function getApiParams(searchParams: URLSearchParams) {
		let videoSource = null;
		for (const source of Object.values(VIDEO_SOURCES)) {
			const value = searchParams.get(source.type);
			if (value) {
				videoSource = { type: source.type, id: value };
			}
		}
		// 支持从 URL 里还原状态筛选
		const statusFilterParam = searchParams.get('status_filter');
		const statusFilter: StatusFilterValue | null =
			statusFilterParam === 'failed' ||
			statusFilterParam === 'succeeded' ||
			statusFilterParam === 'waiting'
				? statusFilterParam
				: null;
		const validationFilterParam = searchParams.get('validation_filter');
		const validationFilter: ValidationFilterValue =
			validationFilterParam === 'skipped' ||
			validationFilterParam === 'invalid' ||
			validationFilterParam === 'normal'
				? validationFilterParam
				: null;
		return {
			query: searchParams.get('query') || '',
			videoSource,
			statusFilter,
			validationFilter,
			createdFrom: searchParams.get('created_from'),
			createdTo: searchParams.get('created_to'),
			pageNum: parseInt(searchParams.get('page') || '0')
		};
	}

	async function loadVideos(
		query: string,
		pageNum: number = 0,
		filter?: { type: string; id: string } | null,
		statusFilter: StatusFilterValue | null = null,
		validationFilter: ValidationFilterValue | null = null,
		createdFrom: string | null = null,
		createdTo: string | null = null
	) {
		loading = true;
		try {
			const params: Record<string, string | number | boolean> = {
				page: pageNum,
				page_size: pageSize
			};
			if (query) {
				params.query = query;
			}
			if (filter) {
				params[filter.type] = parseInt(filter.id);
			}
			if (statusFilter) {
				params.status_filter = statusFilter;
			}
			if (validationFilter) {
				params.validation_filter = validationFilter;
			}
			if (createdFrom) {
				params.created_from = createdFrom;
			}
			if (createdTo) {
				params.created_to = createdTo;
			}
			const result = await api.getVideos(params);
			videosData = result.data;
		} catch (error) {
			console.error('加载视频失败：', error);
			toast.error('加载视频失败', {
				description: (error as ApiError).message
			});
		} finally {
			loading = false;
		}
	}

	$: selectedCount = selectedVideoIds.size;
	$: allVideosSelected = !!videosData?.videos.length && videosData.videos.every((video) => selectedVideoIds.has(video.id));
	$: someVideosSelected = selectedCount > 0 && !allVideosSelected;

	function toggleVideoSelection(id: number) {
		const next = new Set(selectedVideoIds);
		if (next.has(id)) next.delete(id);
		else next.add(id);
		selectedVideoIds = next;
	}

	function toggleAllVideos() {
		if (!videosData) return;
		const next = new Set(selectedVideoIds);
		if (allVideosSelected) {
			for (const video of videosData.videos) next.delete(video.id);
		} else {
			for (const video of videosData.videos) next.add(video.id);
		}
		selectedVideoIds = next;
	}

	async function handleDeleteVideos() {
		if (!selectedVideoIds.size) return;
		deleting = true;
		try {
			const result = await api.deleteVideos([...selectedVideoIds]);
			const data = result.data;
			selectedVideoIds = new Set();
			deleteDialogOpen = false;
			if (data.warnings.length) {
				toast.warning(`已删除 ${data.deleted_count} 个视频，但有文件删除失败`, {
					description: data.warnings.join('；')
				});
			} else {
				toast.success(`已删除 ${data.deleted_count} 个视频及其本地文件`);
			}
			await reloadVideos();
		} catch (error) {
			console.error('删除视频失败：', error);
			toast.error('删除视频失败', { description: (error as ApiError).message });
		} finally {
			deleting = false;
		}
	}

	function getOverallStatus(video: VideoInfo): { text: string; className: string } {
		if (!video.valid) return { text: '失效', className: 'text-muted-foreground' };
		if (!video.should_download) return { text: '跳过', className: 'text-muted-foreground' };
		if (video.download_status.every((status) => status === 7)) return { text: '完成', className: 'text-emerald-600' };
		if (video.download_status.some((status) => status > 0 && status < 7)) return { text: '失败', className: 'text-destructive' };
		return { text: '等待', className: 'text-amber-600' };
	}

	async function reloadVideos() {
		const {
			query,
			currentPage,
			videoSource,
			statusFilter,
			validationFilter,
			createdFrom,
			createdTo
		} = $appStateStore;
		await loadVideos(
			query,
			currentPage,
			videoSource,
			statusFilter,
			validationFilter,
			createdFrom,
			createdTo
		);
	}

	async function handlePageChange(pageNum: number) {
		setCurrentPage(pageNum);
		goto(`/${ToQuery($appStateStore)}`);
	}

	async function handleSearchParamsChange(searchParams: URLSearchParams) {
		const { query, videoSource, pageNum, statusFilter, validationFilter, createdFrom, createdTo } =
			getApiParams(searchParams);
		setAll(query, pageNum, videoSource, statusFilter, validationFilter, createdFrom, createdTo);
		loadVideos(query, pageNum, videoSource, statusFilter, validationFilter, createdFrom, createdTo);
	}

	async function handleResetVideo(id: number, forceReset: boolean) {
		try {
			const result = await api.resetVideoStatus(id, { force: forceReset });
			const data = result.data;
			if (data.resetted) {
				toast.success('重置成功', {
					description: `视频「${data.video.name}」已重置`
				});
				await reloadVideos();
			} else {
				toast.info('重置无效', {
					description: `视频「${data.video.name}」没有失败的状态，无需重置`
				});
			}
		} catch (error) {
			console.error('重置失败：', error);
			toast.error('重置失败', {
				description: (error as ApiError).message
			});
		}
	}

	async function handleClearAndResetVideo(id: number) {
		try {
			const result = await api.clearAndResetVideoStatus(id);
			const data = result.data;
			if (data.warning) {
				toast.warning('清空重置成功', {
					description: data.warning
				});
			} else {
				toast.success('清空重置成功', {
					description: `视频「${data.video.name}」已清空重置`
				});
			}
			await reloadVideos();
		} catch (error) {
			console.error('清空重置失败：', error);
			toast.error('清空重置失败', {
				description: (error as ApiError).message
			});
		}
	}

	async function handleResetAllVideos() {
		resettingAll = true;
		try {
			// 获取筛选参数
			const filterParams = ToFilterParams($appStateStore);
			const result = await api.resetFilteredVideoStatus({
				...filterParams,
				force: forceReset
			});
			const data = result.data;
			if (data.resetted) {
				toast.success('重置成功', {
					description: `已重置 ${data.resetted_videos_count} 个视频和 ${data.resetted_pages_count} 个分页`
				});
				await reloadVideos();
			} else {
				toast.info('没有需要重置的视频');
			}
		} catch (error) {
			console.error('重置失败：', error);
			toast.error('重置失败', {
				description: (error as ApiError).message
			});
		} finally {
			resettingAll = false;
			resetAllDialogOpen = false;
		}
	}

	async function handleUpdateAllVideos(request: UpdateFilteredVideoStatusRequest) {
		updatingAll = true;
		try {
			// 获取筛选参数并合并
			const filterParams = ToFilterParams($appStateStore);
			const fullRequest = {
				...filterParams,
				...request
			};
			const result = await api.updateFilteredVideoStatus(fullRequest);
			const data = result.data;
			if (data.success) {
				toast.success('更新成功', {
					description: `已更新 ${data.updated_videos_count} 个视频和 ${data.updated_pages_count} 个分页`
				});
				await reloadVideos();
			} else {
				toast.info('没有视频被更新');
			}
		} catch (error) {
			console.error('更新失败：', error);
			toast.error('更新失败', {
				description: (error as ApiError).message
			});
		} finally {
			updatingAll = false;
			updateAllDialogOpen = false;
		}
	}

	function getVideoSource(video: VideoInfo): { type: string; name: string } | null {
		if (video.collection_id != null) {
			return sourceMap.get(`collection:${video.collection_id}`) || null;
		}
		if (video.favorite_id != null) {
			return sourceMap.get(`favorite:${video.favorite_id}`) || null;
		}
		if (video.submission_id != null) {
			return sourceMap.get(`submission:${video.submission_id}`) || null;
		}
		if (video.watch_later_id != null) {
			return sourceMap.get(`watch_later:${video.watch_later_id}`) || null;
		}
		return null;
	}

	// 获取筛选条件的显示数组
	function getFilterDescriptionParts(): string[] {
		const state = $appStateStore;
		const parts: string[] = [];
		if (state.query.trim()) {
			parts.push(`搜索词："${state.query}"`);
		}
		if (state.videoSource && videoSources) {
			const sourceType = state.videoSource.type;
			const sourceId = parseInt(state.videoSource.id);
			const sourceConfig = Object.values(VIDEO_SOURCES).find((s) => s.type === sourceType);
			if (sourceConfig) {
				const sourceList = videoSources[sourceType as keyof VideoSourcesResponse] as VideoSource[];
				const source = sourceList.find((s) => s.id === sourceId);
				if (source) {
					parts.push(`${sourceConfig.title}：${source.name}`);
				}
			}
		}
		if (state.statusFilter) {
			const statusLabels = {
				failed: '仅失败',
				succeeded: '仅成功',
				waiting: '仅等待'
			};
			parts.push(`状态：${statusLabels[state.statusFilter]}`);
		}
		if (state.validationFilter) {
			const validationLabels = {
				skipped: '跳过',
				invalid: '失效',
				normal: '有效'
			};
			parts.push(`有效性：${validationLabels[state.validationFilter]}`);
		}
		if (state.createdFrom || state.createdTo) {
			const createdFrom = state.createdFrom?.replace('T', ' ') || '不限';
			const createdTo = state.createdTo?.replace('T', ' ') || '不限';
			parts.push(`创建时间：${createdFrom} 至 ${createdTo}`);
		}
		return parts;
	}

	$: if (videoSourcesLoaded && $page.url.search !== lastSearch) {
		lastSearch = $page.url.search;
		handleSearchParamsChange($page.url.searchParams);
	}

	$: if (videoSources) {
		filters = Object.fromEntries(
			Object.values(VIDEO_SOURCES).map((source) => [
				source.type,
				{
					name: source.title,
					icon: source.icon,
					values: Object.fromEntries(
						(videoSources![source.type as keyof VideoSourcesResponse] as VideoSource[]).map(
							(item) => [item.id, item.name]
						)
					)
				}
			])
		);
		sourceMap.clear();
		for (const source of Object.values(VIDEO_SOURCES)) {
			const sourceList = videoSources[source.type as keyof VideoSourcesResponse] as VideoSource[];
			for (const item of sourceList) {
				sourceMap.set(`${source.type}:${item.id}`, {
					type: source.type,
					name: item.name
				});
			}
		}
	} else {
		filters = null;
		sourceMap.clear();
	}

	onMount(async () => {
		setBreadcrumb([
			{
				label: '视频'
			}
		]);
		videoSources = (await api.getVideoSources()).data;
		videoSourcesLoaded = true;
	});

	$: totalPages = videosData ? Math.ceil(videosData.total_count / pageSize) : 0;
	$: hasFilters = hasActiveFilters($appStateStore);
	$: filterDescriptionParts = videoSources && $appStateStore && getFilterDescriptionParts();
</script>

<svelte:head>
	<title>主页 - Bili Sync</title>
</svelte:head>

<div class="mb-4 flex items-center justify-between">
	<SearchBar
		placeholder="搜索视频标题或 BV 号.."
		value={$appStateStore.query}
		onSearch={(value) => {
			setQuery(value);
			resetCurrentPage();
			goto(`/${ToQuery($appStateStore)}`);
		}}
	></SearchBar>
	<div class="flex items-center gap-3">
		<div class="flex items-center gap-1">
			<span class="text-muted-foreground text-xs">创建时间:</span>
			<CreatedTimeFilter
				createdFrom={$appStateStore.createdFrom}
				createdTo={$appStateStore.createdTo}
				onChange={(createdFrom, createdTo) => {
					setCreatedTimeFilter(createdFrom, createdTo);
					resetCurrentPage();
					goto(`/${ToQuery($appStateStore)}`);
				}}
			/>
		</div>
		<div class="flex items-center gap-1">
			<span class="text-muted-foreground text-xs">有效性:</span>
			<ValidationFilter
				value={$appStateStore.validationFilter}
				onSelect={(value) => {
					setValidationFilter(value);
					resetCurrentPage();
					goto(`/${ToQuery($appStateStore)}`);
				}}
				onRemove={() => {
					setValidationFilter(null);
					resetCurrentPage();
					goto(`/${ToQuery($appStateStore)}`);
				}}
			/>
		</div>
		<!-- 状态筛选 -->
		<div class="flex items-center gap-1">
			<span class="text-muted-foreground text-xs">状态:</span>
			<StatusFilter
				value={$appStateStore.statusFilter}
				onSelect={(value) => {
					setStatusFilter(value);
					resetCurrentPage();
					goto(`/${ToQuery($appStateStore)}`);
				}}
				onRemove={() => {
					setStatusFilter(null);
					resetCurrentPage();
					goto(`/${ToQuery($appStateStore)}`);
				}}
			/>
		</div>
		<!-- 视频源筛选 -->
		<div class="flex items-center gap-1">
			<span class="text-muted-foreground text-xs">来源:</span>
			<DropdownFilter
				{filters}
				selectedLabel={$appStateStore.videoSource}
				onSelect={(type, id) => {
					setAll(
						'',
						0,
						{ type, id },
						$appStateStore.statusFilter,
						$appStateStore.validationFilter,
						$appStateStore.createdFrom,
						$appStateStore.createdTo
					);
					goto(`/${ToQuery($appStateStore)}`);
				}}
				onRemove={() => {
					setAll(
						'',
						0,
						null,
						$appStateStore.statusFilter,
						$appStateStore.validationFilter,
						$appStateStore.createdFrom,
						$appStateStore.createdTo
					);
					goto(`/${ToQuery($appStateStore)}`);
				}}
			/>
		</div>
	</div>
</div>

{#if videosData}
	<div class="mb-6 flex flex-wrap items-center justify-between gap-3">
		<div class="flex items-center gap-6">
			<div class="text-sm font-medium">共 {videosData.total_count} 个视频</div>
			<div class="text-sm font-medium">当前第 {$appStateStore.currentPage + 1} / {totalPages} 页</div>
		</div>
		<div class="flex flex-wrap items-center gap-2">
			{#if selectedCount > 0}
				<Button
					size="sm"
					variant="destructive"
					class="h-8 cursor-pointer text-xs font-medium"
					onclick={() => (deleteDialogOpen = true)}
					disabled={deleting || loading}
				>
					<Trash2Icon class="h-3.5 w-3.5" />
					删除 {selectedCount} 个
				</Button>
			{/if}
			<Button
				size="sm"
				variant="outline"
				class="h-8 cursor-pointer px-2"
				onclick={() => (viewMode = 'card')}
				aria-label="卡片视图"
				aria-pressed={viewMode === 'card'}
			>
				<Grid2X2Icon class="h-3.5 w-3.5" />
			</Button>
			<Button
				size="sm"
				variant="outline"
				class="h-8 cursor-pointer px-2"
				onclick={() => (viewMode = 'list')}
				aria-label="列表视图"
				aria-pressed={viewMode === 'list'}
			>
				<ListIcon class="h-3.5 w-3.5" />
			</Button>
			<Button
				size="sm"
				variant="outline"
				class="h-8 cursor-pointer text-xs font-medium"
				onclick={() => (updateAllDialogOpen = true)}
				disabled={updatingAll || loading}
			>
				<SquarePenIcon class="h-3 w-3" />
				{hasFilters ? '编辑筛选' : '编辑全部'}
			</Button>
			<Button
				size="sm"
				variant="outline"
				class="h-8 cursor-pointer text-xs font-medium"
				onclick={() => (resetAllDialogOpen = true)}
				disabled={resettingAll || loading}
			>
				<RotateCcwIcon class="h-3 w-3 {resettingAll ? 'animate-spin' : ''}" />
				{hasFilters ? '重置筛选' : '重置全部'}
			</Button>
		</div>
	</div>
{/if}

{#if loading}
	<div class="flex items-center justify-center py-16">
		<div class="text-muted-foreground/70 text-sm">加载中...</div>
	</div>
{:else if videosData?.videos.length}
	<div class="mb-4 flex items-center gap-3 border-b pb-3">
		<Checkbox
			checked={allVideosSelected}
			indeterminate={someVideosSelected}
			onclick={toggleAllVideos}
			aria-label="选择当前页视频"
		/>
		<span class="text-muted-foreground text-sm">
			{selectedCount > 0 ? `已选择 ${selectedCount} 个` : '选择当前页视频'}
		</span>
	</div>
	{#if viewMode === 'card'}
		<div class="mb-8 grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 2xl:grid-cols-5">
			{#each videosData.videos as video (video.id)}
				<div class="relative min-w-0">
					<div class="absolute top-3 left-3 z-10 rounded bg-background/90 p-1 shadow-sm">
						<Checkbox
							checked={selectedVideoIds.has(video.id)}
							onclick={() => toggleVideoSelection(video.id)}
							aria-label={`选择 ${video.name}`}
						/>
					</div>
					<VideoCard
						{video}
						source={getVideoSource(video)}
						onReset={async (forceReset: boolean) => await handleResetVideo(video.id, forceReset)}
						onClearAndReset={async () => await handleClearAndResetVideo(video.id)}
					/>
				</div>
			{/each}
		</div>
	{:else}
		<div class="mb-8 overflow-x-auto rounded-md border">
			<table class="w-full text-sm">
				<thead class="bg-muted/50 text-left">
					<tr>
						<th class="w-12 px-3 py-3"></th>
						<th class="px-3 py-3 font-medium">视频</th>
						<th class="px-3 py-3 font-medium">UP主</th>
						<th class="px-3 py-3 font-medium">来源</th>
						<th class="px-3 py-3 font-medium">状态</th>
						<th class="w-24 px-3 py-3"></th>
					</tr>
				</thead>
				<tbody>
					{#each videosData.videos as video (video.id)}
						{@const status = getOverallStatus(video)}
						<tr class="hover:bg-muted/30 border-t">
							<td class="px-3 py-3">
								<Checkbox
									checked={selectedVideoIds.has(video.id)}
									onclick={() => toggleVideoSelection(video.id)}
									aria-label={`选择 ${video.name}`}
								/>
							</td>
							<td class="max-w-[28rem] px-3 py-3">
								<a class="font-medium hover:underline" href={`/video/${video.id}`}>{video.name}</a>
								<div class="text-muted-foreground mt-1 text-xs">{video.bvid}</div>
							</td>
							<td class="text-muted-foreground px-3 py-3">{video.upper_name}</td>
							<td class="px-3 py-3">{getVideoSource(video)?.name || '未知'}</td>
							<td class={`px-3 py-3 font-medium ${status.className}`}>{status.text}</td>
							<td class="px-3 py-3 text-right">
								<Button size="sm" variant="ghost" onclick={() => goto(`/video/${video.id}`)} aria-label="查看详情">
									<InfoIcon class="h-4 w-4" />
								</Button>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{/if}

	<Pagination currentPage={$appStateStore.currentPage} {totalPages} onPageChange={handlePageChange} />
{:else}
	<div class="flex items-center justify-center py-16">
		<div class="space-y-3 text-center">
			<p class="text-muted-foreground text-sm">暂无视频数据</p>
			<p class="text-muted-foreground/70 text-xs">尝试搜索或检查视频来源配置</p>
		</div>
	</div>
{/if}

<AlertDialog.Root bind:open={resetAllDialogOpen}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{hasFilters ? '重置筛选视频' : '重置全部视频'}</AlertDialog.Title>
			<AlertDialog.Description>
				{#if hasFilters}
					确定要重置<strong>符合以下筛选条件</strong>的视频的下载状态吗？<br />
					<div class="bg-muted my-2 rounded-md p-2 text-left">
						{#each filterDescriptionParts as part, index (index)}
							<div><strong>{part}</strong></div>
						{/each}
					</div>
				{:else}
					确定要重置<strong>全部视频</strong>的下载状态吗？<br />
				{/if}
				此操作会将所有的失败状态重置为未开始，<span class="text-destructive font-medium"
					>无法撤销</span
				>。
			</AlertDialog.Description>
		</AlertDialog.Header>

		<div class="py-2">
			<div class="rounded-lg border border-orange-200 bg-orange-50 p-3">
				<div class="mb-2 flex items-center space-x-2">
					<Checkbox id="force-reset-all" bind:checked={forceReset} />
					<Label for="force-reset-all" class="text-sm font-medium text-orange-700"
						>⚠️ 强制重置</Label
					>
				</div>
				<p class="text-xs leading-relaxed text-orange-700">
					除重置失败状态外还会检查修复任务状态的标识位 <br />
					版本升级引入新任务时勾选该选项进行重置，可以允许旧视频执行新任务
				</p>
			</div>
		</div>

		<AlertDialog.Footer>
			<AlertDialog.Cancel
				disabled={resettingAll}
				onclick={() => {
					forceReset = false;
				}}>取消</AlertDialog.Cancel
			>
			<AlertDialog.Action
				onclick={handleResetAllVideos}
				disabled={resettingAll}
				class={forceReset ? 'bg-orange-600 hover:bg-orange-700' : ''}
			>
				{#if resettingAll}
					<RotateCcwIcon class="mr-2 h-4 w-4 animate-spin" />
					重置中...
				{:else}
					{forceReset ? '确认强制重置' : '确认重置'}
				{/if}
			</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<AlertDialog.Root bind:open={deleteDialogOpen}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>删除选中的视频</AlertDialog.Title>
			<AlertDialog.Description>
				确定删除选中的 {selectedCount} 个视频吗？这会删除视频记录、分页记录及对应的本地文件，且无法撤销。
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel disabled={deleting}>取消</AlertDialog.Cancel>
			<AlertDialog.Action
				onclick={handleDeleteVideos}
				disabled={deleting}
				class="bg-destructive hover:bg-destructive/90"
			>
				{#if deleting}<Trash2Icon class="h-4 w-4 animate-pulse" />{/if}
				{deleting ? '删除中...' : '确认删除'}
			</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<FilteredStatusEditor
	bind:open={updateAllDialogOpen}
	{hasFilters}
	loading={updatingAll}
	filterDescriptionParts={filterDescriptionParts || []}
	onsubmit={handleUpdateAllVideos}
/>
