/* ==========================================================================
   asciicut brand website interactivity
   ========================================================================== */
document.addEventListener('DOMContentLoaded', () => {
    initCopyButtons();
    initTabSwitchers();
    initCarousel();
    initInteractiveCutDemo();
});
// ==========================================================================
// Copy to Clipboard Buttons
// ==========================================================================
function initCopyButtons() {
    // Hero command copy button
    const copyHeroBtn = document.getElementById('copy-cmd-btn');
    if (copyHeroBtn) {
        copyHeroBtn.addEventListener('click', () => {
            const textToCopy = "npx asciicut demo.cast";
            navigator.clipboard.writeText(textToCopy).then(() => {
                const copyText = copyHeroBtn.querySelector('.copy-text');
                if (copyText) copyText.textContent = 'Copied!';
                copyHeroBtn.classList.add('copied');

                setTimeout(() => {
                    if (copyText) copyText.textContent = 'Copy';
                    copyHeroBtn.classList.remove('copied');
                }, 2000);
            });
        });
    }
    // Install panel copy buttons
    const copyCodeBtns = document.querySelectorAll('.copy-btn-code');
    copyCodeBtns.forEach(btn => {
        btn.addEventListener('click', () => {
            const textToCopy = btn.getAttribute('data-copy');
            navigator.clipboard.writeText(textToCopy).then(() => {
                const originalText = btn.textContent;
                btn.textContent = 'Copied!';
                btn.classList.add('copied');

                setTimeout(() => {
                    btn.textContent = originalText;
                    btn.classList.remove('copied');
                }, 2000);
            });
        });
    });
}
// ==========================================================================
// Tab Switchers
// ==========================================================================
let playerInitialized = false;
function initTabSwitchers() {
    // Demo section tabs
    const demoTabBtns = document.querySelectorAll('.demo-tab-btn');
    demoTabBtns.forEach(btn => {
        btn.addEventListener('click', () => {
            const tabId = btn.getAttribute('data-tab');

            // Toggle active classes on buttons
            demoTabBtns.forEach(b => b.classList.remove('active'));
            btn.classList.add('active');

            // Toggle active classes on tab contents
            const contents = document.querySelectorAll('.demo-tab-content');
            contents.forEach(c => c.classList.remove('active'));

            const targetContent = document.getElementById(`tab-${tabId}`);
            if (targetContent) {
                targetContent.classList.add('active');
            }
            // Lazy initialize asciinema-player when player tab is opened
            if (tabId === 'live-player' && !playerInitialized) {
                initAsciinemaPlayer();
            }
        });
    });
    // Install section tabs
    const installTabBtns = document.querySelectorAll('.install-tab-btn');
    installTabBtns.forEach(btn => {
        btn.addEventListener('click', () => {
            const installId = btn.getAttribute('data-install');

            // Toggle buttons
            installTabBtns.forEach(b => b.classList.remove('active'));
            btn.classList.add('active');

            // Toggle contents
            const contents = document.querySelectorAll('.install-tab-content-item');
            contents.forEach(c => c.classList.remove('active'));

            const targetContent = document.getElementById(`inst-${installId}`);
            if (targetContent) {
                targetContent.classList.add('active');
            }
        });
    });
}
// ==========================================================================
// Asciinema Player Setup
// ==========================================================================
function initAsciinemaPlayer() {
    if (typeof AsciinemaPlayer !== 'undefined') {
        try {
            AsciinemaPlayer.create(
                'assets/test-dir-ops.cast',
                document.getElementById('player-container'),
                {
                    cols: 80,
                    rows: 24,
                    autoPlay: false,
                    preload: true,
                    loop: true,
                    speed: 1.0,
                    theme: 'asciicut',
                    terminalFontSize: '15px'
                }
            );
            playerInitialized = true;
        } catch (e) {
            console.error("Failed to initialize asciinema player:", e);
        }
    } else {
        console.warn("AsciinemaPlayer library is not loaded.");
    }
}
// ==========================================================================
// Screenshot Carousel
// ==========================================================================
function initCarousel() {
    const indicators = document.querySelectorAll('.carousel-indicator');
    const slides = document.querySelectorAll('.carousel-slide');
    let currentSlide = 0;
    let carouselInterval;
    function showSlide(index) {
        slides.forEach(s => s.classList.remove('active'));
        indicators.forEach(i => i.classList.remove('active'));

        slides[index].classList.add('active');
        indicators[index].classList.add('active');
        currentSlide = index;
    }
    indicators.forEach((ind, index) => {
        ind.addEventListener('click', () => {
            showSlide(index);
            resetAutoPlay();
        });
    });
    function nextSlide() {
        const next = (currentSlide + 1) % slides.length;
        showSlide(next);
    }
    function startAutoPlay() {
        carouselInterval = setInterval(nextSlide, 5000); // Change slide every 5 seconds
    }
    function resetAutoPlay() {
        clearInterval(carouselInterval);
        startAutoPlay();
    }
    if (slides.length > 0) {
        startAutoPlay();
    }
}
// ==========================================================================
// Interactive Timeline Cut Demo Simulator
// ==========================================================================
function initInteractiveCutDemo() {
    const waveformContainer = document.getElementById('waveform-display');
    const filmstripContainer = document.getElementById('filmstrip-display');
    const tracksContainer = document.getElementById('tracks-display');

    const snipBtn = document.getElementById('btn-snip-demo');
    const resetBtn = document.getElementById('btn-reset-demo');
    const composedDuration = document.getElementById('composed-duration');
    const activeSegmentPill = document.getElementById('active-segment-pill');
    const speedVal = document.getElementById('speed-val');
    if (!waveformContainer || !filmstripContainer || !tracksContainer) return;
    // Define timeline regions (40 units total representing 2m 14s total duration)
    const regions = [];
    for (let i = 0; i < 40; i++) {
        let type = 'keep';
        let segmentId = '';
        if (i >= 0 && i <= 8) { type = 's1'; segmentId = 's1'; }
        else if (i >= 9 && i <= 17) { type = 'd1'; segmentId = 'd1'; }
        else if (i >= 18 && i <= 25) { type = 's2'; segmentId = 's2'; }
        else if (i >= 26 && i <= 35) { type = 'd2'; segmentId = 'd2'; }
        else if (i >= 36 && i <= 39) { type = 's3'; segmentId = 's3'; }
        regions.push({ index: i, type, segmentId });
    }
    // Generate Waveform bars
    function generateWaveform() {
        waveformContainer.innerHTML = '';
        regions.forEach(r => {
            const bar = document.createElement('div');
            bar.className = 'waveform-bar';
            bar.setAttribute('data-segment', r.segmentId);

            let h = 0;
            if (r.type === 's1') {
                h = 30 + Math.random() * 25;
                bar.classList.add('kept-peak');
            } else if (r.type === 'd1') {
                h = 4;
                bar.classList.add('dead-valley');
            } else if (r.type === 's2') {
                h = 40 + Math.random() * 20;
                bar.classList.add('kept-peak');
            } else if (r.type === 'd2') {
                h = 4;
                bar.classList.add('dead-valley');
            } else if (r.type === 's3') {
                h = 20 + Math.random() * 10;
                bar.classList.add('kept-peak');
            }
            bar.style.height = `${h}px`;
            waveformContainer.appendChild(bar);
        });
    }
    // Generate Filmstrip thumbnails (6 pieces)
    const thumbsData = [
        { time: '0:05', title: 'welcome', pos: 3, img: 'assets/shot-editor.png' },
        { time: '0:25', title: 'empty', pos: 12, img: 'assets/ui_empty_state_transparent.png' },
        { time: '0:50', title: 'editor', pos: 22, img: 'assets/shot-editor.png' },
        { time: '1:20', title: 'inspector', pos: 29, img: 'assets/shot-inspector.png' },
        { time: '1:45', title: 'preview', pos: 34, img: 'assets/shot-preview.png' },
        { time: '2:05', title: 'export', pos: 38, img: 'assets/shot-export.png' }
    ];
    function generateFilmstrip() {
        filmstripContainer.innerHTML = '';
        thumbsData.forEach(t => {
            const thumb = document.createElement('div');
            thumb.className = 'filmstrip-thumb';
            if (t.pos >= 9 && t.pos <= 17) {
                thumb.setAttribute('data-segment', 'd1');
            } else if (t.pos >= 26 && t.pos <= 35) {
                thumb.setAttribute('data-segment', 'd2');
            } else if (t.pos >= 0 && t.pos <= 8) {
                thumb.setAttribute('data-segment', 's1');
            } else if (t.pos >= 18 && t.pos <= 25) {
                thumb.setAttribute('data-segment', 's2');
            } else {
                thumb.setAttribute('data-segment', 's3');
            }

            thumb.style.backgroundImage = `url(${t.img})`;
            thumb.innerHTML = `<span class="thumb-time">${t.time}</span>`;
            filmstripContainer.appendChild(thumb);
        });
    }
    // Generate Tracks segments
    const trackSegmentsData = [
        { id: 's1', label: 's1 (Init Project)', type: 'keep', left: 0, width: 22.5 },
        { id: 'd1', label: '✂️ Dead Air (valley)', type: 'dead', left: 22.5, width: 22.5 },
        { id: 's2', label: 's2 (Compilation)', type: 'keep', left: 45, width: 20 },
        { id: 'd2', label: '✂️ Dead Air (valley)', type: 'dead', left: 65, width: 25 },
        { id: 's3', label: 's3 (Payoff hold)', type: 'keep', left: 90, width: 10 }
    ];
    function generateTracks() {
        tracksContainer.innerHTML = '';
        trackSegmentsData.forEach(t => {
            const seg = document.createElement('div');
            seg.className = `track-segment ${t.type}`;
            seg.id = `track-${t.id}`;
            seg.style.left = `${t.left}%`;
            seg.style.width = `${t.width}%`;
            seg.textContent = t.label;

            if (t.type === 'keep') {
                seg.addEventListener('click', () => {
                    document.querySelectorAll('.track-segment.keep').forEach(s => s.classList.remove('active'));
                    seg.classList.add('active');
                    activeSegmentPill.textContent = t.id === 's1' ? 's1 (Init project)' : t.id === 's2' ? 's2 (Compilation)' : 's3 (Result Hold)';
                    speedVal.textContent = t.id === 's2' ? '2.5x' : '1.0x';
                });
            }
            tracksContainer.appendChild(seg);
        });
    }
    function renderInitialState() {
        generateWaveform();
        generateFilmstrip();
        generateTracks();

        composedDuration.textContent = '2m 14s';
        activeSegmentPill.textContent = 's1 (Init project)';
        speedVal.textContent = '1.0x';

        snipBtn.disabled = false;
        snipBtn.innerHTML = `
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="6" r="3"></circle><circle cx="6" cy="18" r="3"></circle><line x1="20" y1="4" x2="8.12" y2="15.88"></line><line x1="14.47" y1="14.48" x2="20" y2="20"></line><line x1="8.12" y1="8.12" x2="12" y2="12"></line></svg>
            Auto-Snip Dead Air
        `;
    }
    // Trigger Snip Cut action
    snipBtn.addEventListener('click', () => {
        const deadBars = document.querySelectorAll('.waveform-bar.dead-valley');
        deadBars.forEach(bar => {
            bar.classList.add('snapped');
        });
        const deadThumbs = document.querySelectorAll('.filmstrip-thumb[data-segment="d1"], .filmstrip-thumb[data-segment="d2"]');
        deadThumbs.forEach(thumb => {
            thumb.classList.add('snapped');
        });
        const trackD1 = document.getElementById('track-d1');
        const trackD2 = document.getElementById('track-d2');
        const trackS2 = document.getElementById('track-s2');
        const trackS3 = document.getElementById('track-s3');
        if (trackD1) {
            trackD1.classList.add('snapped');
            trackD1.style.width = '0%';
        }
        if (trackD2) {
            trackD2.classList.add('snapped');
            trackD2.style.width = '0%';
        }
        setTimeout(() => {
            if (trackS2) trackS2.style.left = '22.5%';
            if (trackS3) trackS3.style.left = '42.5%';
        }, 150);
        animateDuration(134, 26, 800);
        activeSegmentPill.textContent = 'Compose complete';
        speedVal.textContent = 'Balanced';

        snipBtn.disabled = true;
        snipBtn.innerHTML = `✂️ Snip Complete! Saved 80%`;
    });
    function animateDuration(startSecs, endSecs, durationMs) {
        const startTime = performance.now();

        function update(currentTime) {
            const elapsed = currentTime - startTime;
            const progress = Math.min(elapsed / durationMs, 1);

            const currentSecs = Math.round(startSecs - progress * (startSecs - endSecs));

            const mins = Math.floor(currentSecs / 60);
            const secs = currentSecs % 60;
            composedDuration.textContent = `${mins}m ${secs.toString().padStart(2, '0')}s`;

            if (progress < 1) {
                requestAnimationFrame(update);
            }
        }
        requestAnimationFrame(update);
    }
    resetBtn.addEventListener('click', () => {
        renderInitialState();
    });
    renderInitialState();
}
