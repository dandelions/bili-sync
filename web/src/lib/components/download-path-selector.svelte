<script lang="ts">
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Button } from '$lib/components/ui/button/index.js';
	import * as Tooltip from '$lib/components/ui/tooltip/index.js';
	import InfoIcon from '@lucide/svelte/icons/info';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import XIcon from '@lucide/svelte/icons/x';
	import CheckIcon from '@lucide/svelte/icons/check';
	import BookmarkPlusIcon from '@lucide/svelte/icons/bookmark-plus';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import { toast } from 'svelte-sonner';
	import { onMount } from 'svelte';
	import { cn } from '$lib/utils.js';

	interface Props {
		id?: string;
		value?: string;
		placeholder?: string;
		label?: string;
		description?: string;
		showTooltip?: boolean;
		tooltipContent?: string;
		existingPaths?: string[];
		disabled?: boolean;
		class?: string;
	}

	let {
		id = 'download-path',
		value = $bindable(''),
		placeholder = '请输入下载路径，例如：/path/to/download',
		label = '下载路径',
		description = '',
		showTooltip = true,
		tooltipContent = '视频/音频文件的保存目标目录。',
		existingPaths = [],
		disabled = false,
		class: className = ''
	}: Props = $props();

	const STORAGE_KEY = 'bili_sync_custom_download_paths';

	let customPaths = $state<string[]>([]);
	let isAdding = $state(false);
	let newPathInput = $state('');
	let mainInputRef = $state<HTMLInputElement | null>(null);
	let addInputRef = $state<HTMLInputElement | null>(null);

	function loadCustomPaths() {
		if (typeof window === 'undefined') return;
		try {
			const saved = localStorage.getItem(STORAGE_KEY);
			if (saved) {
				const parsed = JSON.parse(saved);
				if (Array.isArray(parsed)) {
					customPaths = parsed.filter((p) => typeof p === 'string' && p.trim().length > 0);
				}
			}
		} catch (e) {
			console.error('Failed to load custom download paths:', e);
		}
	}

	function saveCustomPaths(paths: string[]) {
		customPaths = paths;
		if (typeof window === 'undefined') return;
		try {
			localStorage.setItem(STORAGE_KEY, JSON.stringify(paths));
		} catch (e) {
			console.error('Failed to save custom download paths:', e);
		}
	}

	function handleSelect(path: string) {
		value = path;
		toast.success('已填充路径', { description: path });
	}

	function handleAddCustom() {
		const trimmed = newPathInput.trim();
		if (!trimmed) {
			toast.error('目录路径不能为空');
			return;
		}
		if (customPaths.includes(trimmed)) {
			toast.error('该目录已在常用列表中');
			return;
		}
		saveCustomPaths([...customPaths, trimmed]);
		toast.success('已添加常用目录');
		newPathInput = '';
		isAdding = false;
	}

	function handleSaveCurrent() {
		const trimmed = value.trim();
		if (!trimmed) {
			toast.error('当前路径为空，无法保存');
			return;
		}
		if (customPaths.includes(trimmed)) {
			toast.info('该目录已在常用列表中');
			return;
		}
		saveCustomPaths([...customPaths, trimmed]);
		toast.success('已将当前路径保存为常用目录', { description: trimmed });
	}

	function handleDeleteCustom(pathToDelete: string, e: MouseEvent) {
		e.stopPropagation();
		saveCustomPaths(customPaths.filter((p) => p !== pathToDelete));
		toast.success('已移除常用目录');
	}

	// 合并并去重所有可用的常用目录（现有视频源路径 + 用户自定义路径）
	let displayPaths = $derived.by(() => {
		const set = new Set<string>();
		// 优先放入自定义路径
		for (const p of customPaths) {
			if (p.trim()) set.add(p.trim());
		}
		// 再放入已有视频源路径
		for (const p of existingPaths) {
			if (p && p.trim()) set.add(p.trim());
		}
		return Array.from(set);
	});

	onMount(() => {
		loadCustomPaths();
	});
</script>

<div class={cn('space-y-2', className)}>
	<!-- 标题与说明 -->
	<div class="flex items-center justify-between">
		<div class="flex items-center gap-1.5">
			<Label for={id} class="text-sm font-medium">{label}</Label>
			{#if showTooltip}
				<Tooltip.Root>
					<Tooltip.Trigger>
						<InfoIcon class="text-muted-foreground h-3.5 w-3.5" />
					</Tooltip.Trigger>
					<Tooltip.Content>
						<p class="text-xs">{tooltipContent}</p>
					</Tooltip.Content>
				</Tooltip.Root>
			{/if}
		</div>

		{#if value.trim()}
			<button
				type="button"
				onclick={handleSaveCurrent}
				class="text-muted-foreground hover:text-primary flex cursor-pointer items-center gap-1 text-xs transition-colors"
				title="将当前输入路径保存到常用目录"
			>
				<BookmarkPlusIcon class="h-3.5 w-3.5" />
				<span>保存当前路径为常用</span>
			</button>
		{/if}
	</div>

	<!-- 路径输入框 -->
	<div class="relative">
		<Input
			{id}
			bind:ref={mainInputRef}
			type="text"
			bind:value
			{placeholder}
			{disabled}
			class="font-mono text-sm"
		/>
	</div>

	<!-- 常用目录点选区域 -->
	<div class="space-y-1.5 pt-0.5">
		<div class="text-muted-foreground flex items-center justify-between text-xs">
			<span class="font-medium">常用目录（点击直接填充）：</span>
			<div class="flex items-center gap-2">
				{#if !isAdding}
					<button
						type="button"
						onclick={() => {
							isAdding = true;
							newPathInput = '';
						}}
						class="text-primary flex cursor-pointer items-center gap-0.5 font-medium hover:underline"
					>
						<PlusIcon class="h-3 w-3" />
						<span>新增常用目录</span>
					</button>
				{/if}
			</div>
		</div>

		<!-- 目录标签列表 -->
		{#if displayPaths.length > 0}
			<div class="flex flex-wrap gap-1.5">
				{#each displayPaths as path (path)}
					{@const active = value.trim() === path}
					{@const isCustom = customPaths.includes(path)}
					<div
						class={cn(
							'group inline-flex items-center gap-1 rounded-md border px-2 py-1 font-mono text-xs transition-colors select-none',
							active
								? 'bg-primary text-primary-foreground border-primary font-medium shadow-xs'
								: 'bg-secondary/60 hover:bg-secondary text-secondary-foreground hover:border-border border-transparent cursor-pointer'
						)}
					>
						<button
							type="button"
							onclick={() => handleSelect(path)}
							class="flex cursor-pointer items-center gap-1"
							title={active ? '当前选中的路径' : '点击填充此路径'}
						>
							{#if active}
								<CheckIcon class="h-3 w-3" />
							{:else}
								<FolderIcon class="h-3 w-3 text-muted-foreground" />
							{/if}
							<span>{path}</span>
						</button>

						{#if isCustom}
							<button
								type="button"
								onclick={(e) => handleDeleteCustom(path, e)}
								class={cn(
									'ml-0.5 rounded-xs p-0.5 transition-opacity',
									active
										? 'text-primary-foreground/70 hover:bg-primary-foreground/20 hover:text-primary-foreground'
										: 'text-muted-foreground hover:bg-destructive/20 hover:text-destructive opacity-40 group-hover:opacity-100'
								)}
								title="删除此常用目录"
							>
								<XIcon class="h-2.5 w-2.5" />
							</button>
						{/if}
					</div>
				{/each}
			</div>
		{:else if !isAdding}
			<p class="text-muted-foreground text-xs italic">
				暂无常用目录。您可以在上方输入路径后点击「保存当前路径为常用」，或点击右上角「新增常用目录」。
			</p>
		{/if}

		<!-- 新增常用目录表单 -->
		{#if isAdding}
			<div class="bg-muted/40 mt-1 flex items-center gap-2 rounded-lg border p-2">
				<Input
					bind:ref={addInputRef}
					bind:value={newPathInput}
					placeholder="输入常用目录路径，例如：/media/downloads"
					class="h-8 font-mono text-xs"
					onkeydown={(e) => {
						if (e.key === 'Enter') {
							e.preventDefault();
							handleAddCustom();
						} else if (e.key === 'Escape') {
							isAdding = false;
						}
					}}
				/>
				<Button size="sm" class="h-8 text-xs" onclick={handleAddCustom}>
					保存
				</Button>
				<Button
					size="sm"
					variant="ghost"
					class="h-8 text-xs"
					onclick={() => {
						isAdding = false;
					}}
				>
					取消
				</Button>
			</div>
		{/if}
	</div>

	{#if description}
		<p class="text-muted-foreground text-xs">{description}</p>
	{/if}
</div>
