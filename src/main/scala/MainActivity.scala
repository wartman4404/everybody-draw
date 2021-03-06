package com.github.wartman4404.gldraw

import _root_.android.app.Activity
import _root_.android.os.Bundle

import android.widget._
import android.view._
import android.graphics.{SurfaceTexture, Bitmap}
import android.content.{Context, Intent}
import android.content.res.Configuration
import android.app.AlertDialog
import android.support.v4.widget.DrawerLayout
import android.util.DisplayMetrics

import java.io.{BufferedInputStream}
import java.io.{OutputStream, FileOutputStream, BufferedOutputStream}
import java.io.{File, IOException, FileNotFoundException}
import java.util.Date

import android.util.Log

import scala.collection.mutable

import com.ipaulpro.afilechooser.utils.FileUtils

import unibrush.{UniBrush, Layer}

import com.larswerkman.holocolorpicker.{ColorPicker, ScaleBar}

import scala.concurrent.ExecutionContext
import scala.concurrent.Future
import scala.util.{Success,Failure}
import java.util.concurrent.Executors

import PaintControls.UnnamedPicker
import PaintControls.{GLControl, SelectedListener}


class MainActivity extends Activity with TypedActivity with AndroidImplicits {
  import MainActivity._
  import MainActivity.Constants._
  import GLResultTypeDef._

  lazy val content = new TextureView(this)
  lazy val contentframe = findView(TR.textureviewframe)

  lazy val controls = PaintControls(
    brushpicker = findView(TR.brushpicker),
    animpicker = findView(TR.animpicker),
    paintpicker = findView(TR.paintpicker),
    interppicker = findView(TR.interppicker),
    unipicker = findView(TR.unipicker),
    colorpicker = colorPicker,
    scalebar = findView(TR.brush_colorpicker_scalebar)
  )

  lazy val drawerParent = findView(TR.drawer_parent)
  lazy val sidebar = findView(TR.sidebar_parent)
  lazy val drawerToggle = new MotionEventDrawerToggle(
      this, drawerParent, R.string.sidebar_open, R.string.sidebar_close)
  lazy val sidebarAdapter = new SidebarAdapter()
  lazy val undoButton = findView(TR.undo_button)
  lazy val redoButton = findView(TR.redo_button)
  lazy val clearButton = findView(TR.clear_button)
  lazy val loadButton = findView(TR.load_button)
  lazy val saveButton = findView(TR.save_button)
  lazy val colorPicker = findView(TR.brush_colorpicker_main)
  lazy val controlholder = findView(TR.controlholder)

  var newRotation = -1

  var textureThread: Option[TextureSurfaceThread] = None

  private var savedBitmap: Option[Bitmap] = None

  lazy val saveThread = ExecutionContext.fromExecutor(Executors.newSingleThreadExecutor())

  var loadedDrawFiles: Future[LoadedDrawFiles] = null
  var drawerIsOpen = false

  // TODO: actually clean up
  var handlers: Option[MotionEventHandlerPair] = None

  def createTextureThread(handlers: MotionEventHandlerPair)(s: SurfaceTexture, x: Int, y: Int): Unit = {
    val thread = new TextureSurfaceThread(s, handlers.consumer, onTextureThreadStarted(x,y, handlers.producer), onTextureThreadError);
    thread.start()
    Log.i("everybody-draws", "main thread: started thread");
  }

  var undoCount: Int = 0
  var undoPos: Int = 0
  var undoListener: Option[MainUndoListener] = None


  class MainUndoListener() extends UndoCallback() {
    override def undoBufferChanged(newSize: Int): Unit = {
      Log.i("everybody-draws", s"main thread: new undo buffer size: ${newSize}")
      undoCount = newSize
      undoPos = newSize - 1
      runOnUiThread(() => {
        updateUndoButtons()
      })
    }
  }

  def moveUndo(offset: Int) = {
    val newPos = undoPos + offset
    if (newPos >= 0 && newPos < undoCount) {
      for (thread <- textureThread) {
        undoPos = newPos
        updateUndoButtons()
        thread.withGL(gl => {
          thread.loadUndo(gl, newPos)
        })
      }
    }
  }

  def updateUndoButtons() = {
    redoButton.setEnabled(undoCount - undoPos > 1)
    undoButton.setEnabled(undoPos > 0)
  }

  val onTextureThreadStarted = (x: Int, y: Int, producer: MotionEventProducer) => (thread: TextureSurfaceThread) => this.runOnUiThread(() => {
    textureThread = Some(thread)
    val undoCallback = new MainUndoListener()
    this.undoListener = Some(undoCallback)
    thread.beginGL(x, y, onTextureCreated(thread, producer, undoCallback) _, undoCallback)
    ()
  })

  // runs on gl thread
  def onTextureCreated(thread: TextureSurfaceThread, producer: MotionEventProducer, undoCallback: MainUndoListener)(gl: GLInit) = {
    try {
      //val rotation = Rotation.fromSurfaceOrientation(controls.rotation.value, newRotation)
      val rotation = NoRotation
      controls.rotation.value = newRotation
      thread.initScreen(gl, savedBitmap, rotation)
    } catch {
      case e: GLException => {
        val message = "got exception while loading saved bitmap, this should never happen!\n" + e
        Log.e("everybody-draws", "main thread: " + message)
        this.runOnUiThread(() => {
          Toast.makeText(MainActivity.this, message, Toast.LENGTH_LONG).show()
        })
      }
    }

    val undoframes = thread.pushUndoFrame(gl)
    undoCallback.undoBufferChanged(undoframes)

    savedBitmap = None
    thread.startFrames(gl)
    populatePickers(producer, thread, gl)
    thread.setBrushColor(gl, colorPicker.getColor())
    thread.setBrushSize(gl, colorPicker.getScale())
    val listener = new ToggleableMotionEventListener(producer)
    drawerToggle.setMotionEventListener(listener)
    content.setOnTouchListener(listener)
    Log.i("everybody-draws", "main thread: finished texture setup")
  }

  def createViewTouchListener(producer: MotionEventProducer) = {
    new ToggleableMotionEventListener(producer)
  }

  override def onCreate(bundle: Bundle) {
    Log.i("everybody-draws", "main thread: oncreate")
    System.loadLibrary("gl-stuff")

    super.onCreate(bundle)
    setContentView(R.layout.activity_main)
    
    val display = getWindowManager().getDefaultDisplay()
    val outMetrics = new DisplayMetrics()
    display.getMetrics(outMetrics)
    val density  = getResources().getDisplayMetrics().density
    val dpHeight = (outMetrics.heightPixels / density).asInstanceOf[Int]
    val dpWidth  = (outMetrics.widthPixels / density).asInstanceOf[Int]
    newRotation = display.getRotation()

    handlers = Some(MotionEventHandlerPair.init(dpHeight, dpWidth))

    // Trigger off-thread resource enumeration.
    // TODO: what placement gives the fastest startup time?
    // TODO: consider using resources rather than assets, so no enumeration is needed
    // TODO: consider laziness, only populating the needed views
    // TODO: consider recycling a single gridview, they're not cheap
    loadDrawFiles()
    
    updateUndoButtons()
    undoButton.setOnClickListener(() => moveUndo(-1))
    redoButton.setOnClickListener(() => moveUndo(1))

    saveButton.setOnClickListener(saveFile _)
    loadButton.setOnClickListener(loadFile _)
    clearButton.setOnClickListener(() => this.clearScreen())

    val scaleBar = controls.scalebar.scale
    colorPicker.addSaturationBar(findView(TR.brush_colorpicker_saturationbar))
    colorPicker.addValueBar(findView(TR.brush_colorpicker_valuebar))
    colorPicker.addScaleBar(scaleBar)
    colorPicker.setShowOldCenterColor(false)
    colorPicker.setOnColorChangedListener(new ColorPicker.OnColorChangedListener() {
      override def onColorChanged(color: Int) = {
        textureThread.foreach(t => t.withGL(gl => t.setBrushColor(gl, color)))
      }
    })
    scaleBar.setOnScaleChangedListener(new ScaleBar.OnScaleChangedListener() {
      override def onScaleChanged(size: Float) = {
        textureThread.foreach(t => t.withGL(gl => t.setBrushSize(gl, size)))
      }
    })

    drawerParent.setDrawerListener(drawerToggle)
    getActionBar().setDisplayHomeAsUpEnabled(true)
    getActionBar().setHomeButtonEnabled(true)

    // TODO: deal with rotation better
    Option(bundle) match {
      case Some(inState) => {
        Log.i("everybody-draws", "main thread: restoring from bundle")
        savedBitmap = Option(inState.getParcelable("screen"))
        controls.load(inState)
      }
      case None => {
        loadFromFile()
      }
    }
  }

  override protected def onPostCreate(bundle: Bundle) = {
    super.onPostCreate(bundle)
    drawerToggle.syncState()
  }

  override def onConfigurationChanged(config: Configuration) = {
    super.onConfigurationChanged(config)
    drawerToggle.onConfigurationChanged(config)
  }

  override def onOptionsItemSelected(item: MenuItem): Boolean = {
    if (drawerToggle.onOptionsItemSelected(item)) true
    else item.getItemId() match {
      case R.id.menu_save => saveFile()
      case R.id.menu_load => loadFile()
      //case R.id.menu_replay => startReplay()
      case R.id.menu_clear => this.clearScreen()
      case R.id.menu_credits => Credits.displayCredits(this)
      case R.id.menu_debug => showDebugMessagebox()
      //case R.id.menu_rotate => debugRotate()
      case _ => return super.onOptionsItemSelected(item)
    }
    true
  }

  def debugRotate() = {
    for (thread <- textureThread) {
      thread.getBitmap((gl, bitmap) => {
        thread.initScreen(gl, Some(bitmap), new Rotation(1))
      })
    }
  }

  override def onStart() = {
    Log.i("everybody-draws", "main thread: onStart")
    super.onStart()
    loadDrawFiles()
    handlers.foreach(h => {
        content.setSurfaceTextureListener(new TextureListener(createTextureThread(h) _))
        contentframe.addView(content)
      })

  }
  
  // FIXME the texture thread might not be ready yet
  // although, i guess onTextureCreated handles that case?
  override def onResume() = {
    super.onResume()
    textureThread.foreach(_.startFrames())
    Log.i("everybody-draws", "main thread: resumed!")
  }

  override def onPause() = {
    textureThread.foreach(_.stopFrames())
    super.onPause()
    prepareForSave()
    Log.i("everybody-draws", "main thread: paused!")
  }

  protected override def onSaveInstanceState(outState: Bundle) = {
    //Log.i("everybody-draws", "main thread: onSaveInstanceState")
    super.onSaveInstanceState(outState)
    savedBitmap.synchronized {
      savedBitmap.foreach(bitmap => {
          outState.putParcelable("screen", bitmap)
        })
    }
    controls.save(outState)
  }

  override protected def onStop() = {
    super.onStop()
    Log.i("everybody-draws", "main thread: onStop");
    content.setOnTouchListener(null)
    // (textureview does its own cleanup, see SurfaceTextureListener.onSurfaceTextureDestroyed())
    // TODO: is this the right order? probably not
    contentframe.removeAllViews()
    saveLocalState()
    finishEGLCleanup()
    loadedDrawFiles = null
    // TODO: is this necessary?
    textureThread.foreach(_.join())
    textureThread = None
  }

  override protected def onDestroy() = {
    super.onDestroy()
    handlers.foreach(MotionEventHandlerPair.destroy _)
    handlers = None
  }

  private def prepareForSave() = {
    for (thread <- textureThread) {
      textureThread.foreach(thread => savedBitmap = Some(thread.getBitmapSynchronized()))
    }
    controls.updateState()
  }

  private def saveBitmapToFile(bitmap: Bitmap, out: OutputStream): Unit = {
    try {
      if (!bitmap.isRecycled()) {
        bitmap.compress(Bitmap.CompressFormat.PNG, 90, out)
        out.flush()
      } else {
        Log.e("everybody-draws", "main thread: tried to save recycled bitmap!")
      }
    } catch {
      case e: IOException => {
        Log.i("everybody-draws", "main thread: saving to file failed: %s".format(e))
      }
    }
  }

  private def savePickersToFile() = {
    try {
      val out2 = MainActivity.this.openFileOutput("status", Context.MODE_PRIVATE)
      controls.save(out2)
    } catch {
      case e: IOException => {
        Log.e("everybody-draws", "main thread: saving to file failed: %s".format(e))
      }
    }
  }

  private def loadFromFile() = {
    Log.i("everybody-draws", "main thread: loading from file")
    try {
      // TODO: don't do this on the main thread
      StateSaveLock.synchronized {
        val input = new BufferedInputStream(MainActivity.this.openFileInput("screen"))
        DrawFiles.withCloseable(input) {
          savedBitmap = Some(DrawFiles.decodeBitmap(Bitmap.Config.ARGB_8888)(input))
          val input2 = MainActivity.this.openFileInput("status")
          controls.load(input2)
          input2.close()
        }
      }
    } catch {
      case e: FileNotFoundException => { }
      case e @ (_: IOException | _: GLException) => { 
        Log.e("everybody-draws", "main thread: loading from file failed: %s".format(e))
      }
    }
  }

  private def saveLocalState() = {
    savedBitmap.foreach(bitmap => {
        Future {
          StateSaveLock.synchronized {
            val out = new BufferedOutputStream(
              MainActivity.this.openFileOutput("screen", Context.MODE_PRIVATE))
            try {
              DrawFiles.withCloseable(out) {
                saveBitmapToFile(bitmap, out)
              }
            } catch { case e: Exception => {
              Log.e("everybody-draws", "main thread: failed to save screen bitmap: " + e)
            } }
          }
        }(saveThread)
      })
    savePickersToFile()
  }

  def populatePicker[U](picker: UnnamedPicker[U], arr: Array[DrawFiles.Readable[U]], cb: (GLInit, U)=>Unit, thread: TextureSurfaceThread) = {
    val adapter = new LazyPicker(this, thread, arr)
    picker.setAdapter(adapter)
    picker.setListener((view: View, pos: Int) => {
      thread.withGL(gl => adapter.getState(pos, gl) match {
        case Right(value) => cb(gl, value)
        case Left(errmsg) => {
          MainActivity.this.runOnUiThread(() => {
            Toast.makeText(MainActivity.this, "unable to load item!\n" + errmsg, Toast.LENGTH_LONG).show()
            picker.control.setSelection(0)
            adapter.notifyDataSetChanged()
            ()
          })
        }
      })
    })
  }

  def unloadInterpolatorSynchronized(thread: TextureSurfaceThread, producer: MotionEventProducer, gl: GLInit) = {
    val notify = new Object()
    notify.synchronized {
      runOnUiThread(() => {
        MotionEventProducer.nativePauseMotionEvent(producer)
        //Log.i("everybody-draws", "main thread: loading interpolator - paused motion events")
        notify.synchronized {
          notify.notify()
        }
      })
      //Log.i("everybody-draws", "main thread: loading interpolator - waiting for pause")
      notify.wait()
    }
    //Log.i("everybody-draws", "main thread: loading interpolator - finishing lua script")
    try {
      thread.finishLuaScript(gl)
    }
    catch { case _: LuaException => { } }
  }

  def loadInterpolatorSynchronized(thread: TextureSurfaceThread, producer: MotionEventProducer) =
  (gl: GLInit, script: LuaScript) => {
    unloadInterpolatorSynchronized(thread, producer, gl)
    thread.setInterpScript(gl, script)
  }

  def populatePickers(producer: MotionEventProducer, thread: TextureSurfaceThread, gl: GLInit) = {
    // TODO: maybe make the save thread load from disk and then hand off to the gl thread?
    // also, have it opportunistically load at least up to that point

    def populatePickersWithFiles(drawfiles: LoadedDrawFiles) = {
      MainActivity.this.runOnUiThread(() => {
        // TODO: make hardcoded shaders accessible a better way
        val interpLoader = loadInterpolatorSynchronized(thread, producer)
        Log.i("everybody-draws", s"main thread: got ${drawfiles.brushes.length} brushes, ${drawfiles.anims.length} anims, ${drawfiles.paints.length} paints, ${drawfiles.interpscripts.length} interpolation scripts")
        populatePicker(controls.brushpicker, drawfiles.brushes, loadBrush(thread), thread)
        populatePicker(controls.animpicker, drawfiles.anims,  thread.setAnimShader _, thread)
        populatePicker(controls.paintpicker, drawfiles.paints,  thread.setPointShader _, thread)
        populatePicker(controls.interppicker, drawfiles.interpscripts,  interpLoader, thread)
        populatePicker(controls.unipicker, drawfiles.unibrushes, loadUniBrush(thread, producer), thread)
        controls.copypicker.value = thread.outputShader
        controls.restoreState()
      })
    }

    implicit val executionContext = saveThread
    loadedDrawFiles.onComplete {
      case Success(drawfiles) => {
        populatePickersWithFiles(drawfiles)
      }
      case Failure(err) => {
        val msg = "Something went wrong while loading your custom paint files:\n" + err;
        Log.e("everybody-draws", "main thread: " + msg)
        err.printStackTrace()
        MainActivity.this.runOnUiThread(() => {
          Toast.makeText(MainActivity.this, msg, Toast.LENGTH_LONG).show()
        })
        Future { new LoadedDrawFiles(MainActivity.this, false) }.onComplete {
          case Success(drawfiles) => {
            populatePickersWithFiles(drawfiles)
          }
          case Failure(err) => {
            val msg = "Something went wrong while loading the default paint files.  This should never happen!\n" + err;
            Log.e("everybody-draws", "main thread: " + msg)
            err.printStackTrace()
            MainActivity.this.runOnUiThread(() => {
              Toast.makeText(MainActivity.this, msg, Toast.LENGTH_LONG).show()
            })
          }
        }
      }
    }
  }

  // TODO: fewer callbacks
  def loadUniBrushControls(unibrush: UniBrush) = {
    runOnUiThread(() => {
      sidebarAdapter.updateUnibrush(unibrush)
    })
  }

  def loadBrush(thread: TextureSurfaceThread) = (gl: GLInit, bmtx: Texture) => {
    thread.setBrushTexture(gl, bmtx)
    runOnUiThread(() => {
      colorPicker.setNewCenterBitmap(bmtx.bitmap)
    })
  }

  def loadUniBrush(thread: TextureSurfaceThread, producer: MotionEventProducer) =
  (gl: GLInit, unibrush: UniBrush) => {
    Log.i("everybody-draws", "main thread: loading unibrush")
    def getSelectedValue[T](picker: GLControl[T]): Option[T] = {
      // return None if the control is already active, or we're trying to restore a missing value
      // TODO: the missing-value part is probably busted
      if (picker.enabled) {
        None
      } else {
        val tmp: GLStoredResult[T] = picker.currentValue(gl)
        tmp match {
          case Left(msg) => {
            runOnUiThread(() => {
              Toast.makeText(MainActivity.this, "unable to load old control!" + msg, Toast.LENGTH_LONG).show()
            })
            Log.e("everybody-draws", s"main thread: unable to load old control: ${msg}")
            None
          }
          case Right(value) => {
            val tmp: T = value
            Some(tmp)
          }
        }
      }
    }
    val brush: Option[Texture] = unibrush.brush.orElse(getSelectedValue(controls.brushpicker))
    val anim = unibrush.baseanimshader.orElse(getSelectedValue(controls.animpicker))
    val point = unibrush.basepointshader.orElse(getSelectedValue(controls.paintpicker))
    val copy = unibrush.basecopyshader.orElse(getSelectedValue(controls.copypicker))
    val interp = unibrush.interpolator.orElse(getSelectedValue(controls.interppicker))
    
    // Unconditionally call ondone() in the interpolator to write layers, etc
    // This runs the old interpolator and so must run under the old state.
    unloadInterpolatorSynchronized(thread, producer, gl)
    thread.clearLayers(gl)
    for (layer <- unibrush.layers) {
      thread.addLayer(gl, layer.copyshader, layer.pointshader, layer.pointsrc)
    }
    brush.foreach(thread.setBrushTexture(gl, _))
    anim.foreach(thread.setAnimShader(gl, _))
    point.foreach(thread.setPointShader(gl, _))
    copy.foreach(thread.setCopyShader(gl, _))
    interp.foreach(thread.setInterpScript(gl, _))
    loadUniBrushControls(unibrush) // now that we're done, update which controls are enabled
    ()
  }

  override def onCreateOptionsMenu(menu: Menu): Boolean = {
    getMenuInflater.inflate(R.menu.main, menu)
    true
  }

  def finishEGLCleanup() {
    textureThread.foreach(thread => {
        thread.cleanupGL()
      })
  }

  def loadFile() {
    val chooser = Intent.createChooser(FileUtils.createGetContentIntent(), "Pick a source image")
    startActivityForResult(chooser, ACTIVITY_CHOOSE_IMAGE) 
  }

  def saveFile() = {
    val name = new Date().toString() + ".png"
    Toast.makeText(this, "Saving as \"%s\"...".format(name), Toast.LENGTH_LONG).show()
    textureThread.foreach(thread => {
        thread.getBitmap((gl, b) => {
            Future {
              val outfile = new File(getExternalFilesDir(null), name)
              try {
                val outstream = new BufferedOutputStream(new FileOutputStream(outfile))
                DrawFiles.withCloseable(outstream) {
                  saveBitmapToFile(b, outstream)
                }
              } catch { case _: Exception => { } }
            }(saveThread)
          })
      })
  }

  protected override def onActivityResult(requestCode: Int, resultCode: Int, data: Intent) = requestCode match {
    case ACTIVITY_CHOOSE_IMAGE => {
      if (resultCode == Activity.RESULT_OK) {
        val path = FileUtils.getPath(this, data.getData())
        val bitmap = (try {
          val unread = new DrawFiles.Unread(DrawFiles.FileSource, new DrawFiles.BitmapReader(Bitmap.Config.ARGB_8888))
          Some(unread.read(path).content)
        } catch {
          case e: Exception => {
            Toast.makeText(MainActivity.this, s"Unable to load image ${path}: ${e.getMessage()}", Toast.LENGTH_LONG).show()
            None
          }
        })
        for (b <- bitmap; thread <- textureThread) {
          thread.withGL(gl => {
            val success = try {
              thread.drawBitmap(gl, b)
              true
            } catch {
              case e: GLException => {
                runOnUiThread(() => {
                  Toast.makeText(MainActivity.this, s"Unable to load image ${path}: ${e.getMessage()}", Toast.LENGTH_LONG).show()
                })
                false
              }
            }
            if (success) {
              thread.clearUndoFrames(gl)
              val frames = thread.pushUndoFrame(gl)
              undoListener.foreach(_.undoBufferChanged(frames))
            }
          })
        }
      }
    }
    case _ => {
      Log.e("everybody-draws", s"main thread: got unidentified activity result: ${resultCode}, request code ${requestCode}, data: ${data}")
    }
  }

  def onTextureThreadError(e: Exception) = MainActivity.this.runOnUiThread(() => {
      val prefix = (
        e match {
          case _: LuaException => {
            val picker = if (controls.interppicker.enabled) controls.interppicker else controls.unipicker
            picker.control.setSelection(0)
            "An error occurred in the interpolator:\n" 
          }
          case _ => "An error occurred:\n" 
        })
      Toast.makeText(MainActivity.this, prefix + e.getMessage(), Toast.LENGTH_LONG).show()
    })

  def startReplay() = {
    for (thread <- textureThread) {
      Log.i("everybody-draws", "main thread: starting replay...")
      thread.beginReplay()
    }
  }
  
  def showControl(pos: Int) = {
    controlholder.setVisibility(View.VISIBLE)
    drawerParent.closeDrawer(sidebar)
  }
  def hideControls() = {
    controlholder.setVisibility(View.INVISIBLE)
    drawerParent.closeDrawer(sidebar)
  }

  def showDebugMessagebox() {
    for (thread <- textureThread) thread.withGL(gl => {
       import GLSourceable._

       def getGL[T](gl: GLInit, control: GLControl[T]) = control.currentValue(gl).right.toOption
       def shaderSource(name: String, src: (String, String)) = {
         SyntaxHighlightListAdapter.ShaderSource(name, src._1, src._2)
       }
       def luaSource(name: String, src: String) = {
         SyntaxHighlightListAdapter.LuaSource(name, src)
       }

       val unisrc = 
         getGL(gl, controls.unipicker).getOrElse(UniBrush(None, None, None, None, None, Array()))
       val anim = unisrc.baseanimshader.orElse(getGL(gl, controls.animpicker))
       val copy = unisrc.basecopyshader.orElse(getGL(gl, controls.copypicker))
       val paint = unisrc.basepointshader.orElse(getGL(gl, controls.paintpicker))
       val interp = unisrc.interpolator.orElse(getGL(gl, controls.interppicker))

       val animsrc = anim.map(_.getSource(gl)).getOrElse(("", ""))
       val copysrc = copy.map(_.getSource(gl)).getOrElse(("", ""))
       val paintsrc = paint.map(_.getSource(gl)).getOrElse(("", ""))
       val interpsrc = interp.map(_.getSource(gl)).getOrElse("")

       val layerviews = unisrc.layers.zipWithIndex.flatMap { case (layer, idx) => {
         val name = s"Layer ${(idx + 1)}"
         val result: Array[SyntaxHighlightListAdapter.ShaderSource] =
         Array(
           if (!layer.pointshader.isDefault)
             Some(shaderSource(s"${name} paint shader", layer.pointshader.getSource(gl)))
             else None,
           if (!layer.copyshader.isDefault)
             Some(shaderSource(s"${name} copy shader", layer.copyshader.getSource(gl)))
             else None
         ).flatten
         result
       }}

       val sourceviews: Array[SyntaxHighlightListAdapter.Sources] = Array(
         shaderSource("Base animation shader", animsrc),
         shaderSource("Base copy shader", copysrc),
         shaderSource("Base paint shader", paintsrc),
         luaSource("Base interpolator", interpsrc)
       ) ++ layerviews

       MainActivity.this.runOnUiThread(() => {
         val list = new ListView(this)
         list.setAdapter(new SyntaxHighlightListAdapter(this, sourceviews))
         new AlertDialog.Builder(this)
         .setView(list)
         .setTitle("debug")
         .setPositiveButton("Done", () => {})
         .show()
         ()
       })
    })
  }

  def loadDrawFiles() {
    if (loadedDrawFiles == null) {
      loadedDrawFiles = Future {
        new LoadedDrawFiles(this, true)
      }(saveThread)
    }
  }

  def clearScreen() {
    for (thread <- textureThread) {
      thread.withGL(gl => {
        thread.clearScreen(gl)
        val undo = thread.pushUndoFrame(gl)
        undoListener.foreach(_.undoBufferChanged(undo))
      })
    }
  }


  class SidebarAdapter() extends BaseAdapter {
    import SidebarAdapter._
    val inflater = LayoutInflater.from(MainActivity.this)

    // these aren't actually used atm, so don't lock resources to get them
    //private val names = MainActivity.this.getResources().getStringArray(R.array.sidebar_titles)
    private val names = Array("Brush Texture", "Animation", "Paint", "Interpolator", "Unibrushes", "Hide Controls", "Overlay")

    // must match order of viewflipper children
    val sidebarControls = Array (
      new SidebarEntryPicker(names(0), controls.brushpicker, (u: UniBrush) => u.brush),
      new SidebarEntryPicker(names(1), controls.animpicker, (u: UniBrush) => u.baseanimshader),
      new SidebarEntryPicker(names(2), controls.paintpicker, (u: UniBrush) => u.basepointshader),
      new SidebarEntryPicker(names(3), controls.interppicker, (u: UniBrush) => u.interpolator),
      new SidebarEntryPicker(names(4), controls.unipicker, (u: UniBrush) => None),
      new SidebarEntryHider(names(5))
    )
    val copyShaderControl = new SidebarHiddenEntryPicker(names(6), controls.copypicker, (u: UniBrush) => u.basecopyshader)
    override def areAllItemsEnabled = false
    override def isEnabled(pos: Int) = sidebarControls(pos).enabled
    override def getCount = sidebarControls.length
    override def getViewTypeCount() = 1
    override def getItem(pos: Int) = sidebarControls(pos)
    override def getItemId(pos: Int) = pos
    override def getView(pos: Int, convertView: View, parent: ViewGroup): View = {
      val view = if (convertView == null) {
        inflater.inflate(android.R.layout.simple_list_item_activated_1, parent, false)
      } else {
        convertView
      }
      val name = view.findViewById(android.R.id.text1).asInstanceOf[TextView]
      val control = sidebarControls(pos)
      val enabled = control.enabled
      name.setText(control.name)
      name.setEnabled(enabled)
      view.setEnabled(enabled)
      view
    }

    def updateUnibrush(unibrush: UniBrush) = {
      for (control <- sidebarControls) {
        control.updateForUnibrush(unibrush)
      }
      copyShaderControl.updateForUnibrush(unibrush)

      this.notifyDataSetChanged()
    }
  }
  object SidebarAdapter {
    trait SidebarEntry {
      def updateForUnibrush(u: UniBrush): Unit
      def enabled: Boolean
      def name: String
    }
    class SidebarHiddenEntryPicker[T](val name: String, picker: GLControl[_], getUnibrushValue: (UniBrush) => Option[T]) extends SidebarEntry {
      override def enabled = picker.enabled
      override def updateForUnibrush(u: UniBrush) = {
        //val oldstate = enabled
        picker.enabled = getUnibrushValue(u).isEmpty
        //Log.i("everybody-draws", s"main thread: ${if (enabled) "enabling" else "disabling"} control ${name} for unibrush (was: ${if (oldstate) "enabled" else "disabled"})")
      }
    }
    class SidebarEntryPicker[T](name: String, picker: GLControl[_] with SelectedListener, getUnibrushValue: (UniBrush) => Option[T])
    extends SidebarHiddenEntryPicker[T](name, picker, getUnibrushValue) {
      override def updateForUnibrush(u: UniBrush) = {
        super.updateForUnibrush(u)
        picker.control.setEnabled(enabled)
      }
    }

    class SidebarEntryHider(val name: String) extends SidebarEntry {
      override def enabled = true
      override def updateForUnibrush(u: UniBrush) = { }
    }
  }
}

object MainActivity {

  object Constants {
    final val ACTIVITY_CHOOSE_IMAGE = 0x1;
  }

  class ToggleableMotionEventListener(producer: MotionEventProducer)
  extends View.OnTouchListener {
    def setForwardEvents(forwardEvents: Boolean): Unit = {
      this.forwardEvents = forwardEvents
    }
    final private var forwardEvents = true
    override def onTouch(v: View, evt: MotionEvent) = {
      if (forwardEvents) MotionEventProducer.nativeAppendMotionEvent(producer, evt)
      true
    }
  }

  class MotionEventDrawerToggle(activity: Activity, layout: DrawerLayout, openRes: Int, closeRes: Int)
  extends android.support.v7.app.ActionBarDrawerToggle(activity, layout, openRes, closeRes) {
    private final var motionEventListener: Option[ToggleableMotionEventListener] = None
    def setMotionEventListener(listener: ToggleableMotionEventListener): Unit = {
      motionEventListener = Some(listener)
    }
    // onDrawerClosed doesn't get called consistently when sliding out the drawer partway and letting go
    // so we have to track the drawer's current state instead
    private var drawerClosed = false
    override def onDrawerClosed(view: View) = {
      // just to be sure
      motionEventListener.foreach(_.setForwardEvents(true))
      drawerClosed = true
      super.onDrawerClosed(view)
    }
    override def onDrawerOpened(view: View) = {
      drawerClosed = false
    }
    // onDrawerStateChanged does get called consistently, and always after onDrawerClosed()/onDrawerOpened()
    override def onDrawerStateChanged(newState: Int) = {
      val newForwarding = newState match {
        case DrawerLayout.STATE_IDLE => drawerClosed
        case _ => false
      }
      motionEventListener.foreach(_.setForwardEvents(newForwarding))
      super.onDrawerStateChanged(newState)
    }
  }

  class TextureListener(callback: (SurfaceTexture, Int, Int)=>Unit) extends TextureView.SurfaceTextureListener {

    def onSurfaceTextureAvailable(st: android.graphics.SurfaceTexture,  w: Int, h: Int): Unit = {
      callback(st, w, h)
    }
    def onSurfaceTextureDestroyed(st: android.graphics.SurfaceTexture): Boolean = {
      Log.i("everybody-draws", "main thread: got onsurfacetexturedestroyed callback!")
      true
    }
    def onSurfaceTextureSizeChanged(st: android.graphics.SurfaceTexture, w: Int, h: Int): Unit = { }
    def onSurfaceTextureUpdated(st: android.graphics.SurfaceTexture): Unit = { }
  }

  class FrameListener extends SurfaceTexture.OnFrameAvailableListener {
    def onFrameAvailable(st: android.graphics.SurfaceTexture): Unit = { }
  }

  abstract class NamedSidebarControl(val name: String) {
    override def toString() = name
    def onClick(pos: Int)
  }

  val StateSaveLock = new Object()

  val NoRotation = new Rotation(0)
  class Rotation(private val i: Int) extends AnyVal { }
  object Rotation {
    def fromSurfaceOrientation(oldOrientation: Int, newOrientation: Int) = {
      Log.i("everybody-draws", s"main thread: old rotation: ${oldOrientation}, new rotation: ${newOrientation}")
      if (oldOrientation == -1) {
        new Rotation(newOrientation)
      }
      val rot = newOrientation - oldOrientation
      new Rotation((rot + 4) & 0x3)
    }
  }
}
